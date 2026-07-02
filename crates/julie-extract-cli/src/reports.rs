use std::io::{self, Write};
use std::path::Path;

use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportInput, ReportMode,
    ReportOperation, ReportProfile, ReportRevision, ReportStatus, RowDomainCounts, ToolReport,
};
use julie_extract_artifact::writer::{ArtifactSpoolError, ArtifactWriteError};
use serde_json::{Value, json};

use crate::discovery::DiscoveryError;
use crate::extraction::{ExtractFileError, ExtractFileErrorKind};
use crate::paths::PathPolicyError;

pub(crate) struct CommandOutcome {
    pub(crate) exit_code: u8,
    report: Report,
    json: bool,
    report_stream: ReportStream,
}

#[derive(Clone, Copy)]
pub(crate) enum ReportStream {
    Stdout,
    Stderr,
}

pub(crate) struct CommandError {
    pub(crate) diagnostic: ReportDiagnostic,
    pub(crate) exit_code: u8,
}

pub(crate) fn extract_error_outcome(
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

pub(crate) fn extract_error_diagnostic(error: &ExtractFileError) -> ReportDiagnostic {
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

pub(crate) fn discovery_error_diagnostic(error: &DiscoveryError) -> ReportDiagnostic {
    diagnostic(
        ReportCode::ReadFailed,
        error.message.clone(),
        Some(error.path.clone()),
        Some(error.root_relative_path.clone()),
        true,
        json!({}),
    )
}

pub(crate) fn write_error_outcome(
    error: ArtifactWriteError,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
    json_report: bool,
) -> CommandOutcome {
    write_error_outcome_with_profile(error, operation, mode, input, json_report, None)
}

pub(crate) fn write_error_outcome_with_profile(
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

pub(crate) fn spool_error_outcome(
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

pub(crate) fn base_report(
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
        structural_fact_patterns: None,
    }
}

pub(crate) fn artifact_input(
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

pub(crate) fn path_error_outcome(
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
pub(crate) struct PathErrorInput<'a> {
    pub(crate) db_path: Option<&'a Path>,
    pub(crate) root_path: Option<&'a Path>,
    pub(crate) file_path: Option<&'a Path>,
    pub(crate) root_relative_path: Option<&'a str>,
}

pub(crate) fn path_error_outcome_with_paths(
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

pub(crate) trait ReportBuilder {
    fn with_artifact(self, artifact: ArtifactReport) -> Self;
    fn with_revision(self, revision: ReportRevision) -> Self;
    fn with_totals(self, totals: RowDomainCounts) -> Self;
    fn with_profile(self, profile: ReportProfile) -> Self;
    fn with_error(self, error: ReportDiagnostic) -> Self;
    fn with_warning(self, warning: ReportDiagnostic) -> Self;
    fn with_languages(self, languages: Value) -> Self;
    fn with_structural_fact_patterns(self, structural_fact_patterns: Value) -> Self;
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

    fn with_structural_fact_patterns(mut self, structural_fact_patterns: Value) -> Self {
        self.structural_fact_patterns = Some(structural_fact_patterns);
        self
    }
}

pub(crate) fn outcome(
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

pub(crate) fn command_error(
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

pub(crate) fn diagnostic(
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

pub(crate) fn write_outcome(outcome: &CommandOutcome) {
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

pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string()
}
