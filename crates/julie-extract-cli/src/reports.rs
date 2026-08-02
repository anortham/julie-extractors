use std::io::{self, Write};
use std::path::Path;

use julie_extract_artifact::model::{ReferenceSiteConflicts, WriteResult};
use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportInput, ReportMode,
    ReportOperation, ReportProfile, ReportRevision, ReportStatus, RowDomainCounts, ToolReport,
};
use julie_extract_artifact::writer::{ArtifactSpoolError, ArtifactWriteError};
use serde_json::{Value, json};

use crate::resolution::ResolutionReport;

use crate::discovery::DiscoveryError;
use crate::extraction::{ExtractFileError, ExtractFileErrorKind};
use crate::paths::PathPolicyError;

pub(crate) struct CommandOutcome {
    pub(crate) exit_code: u8,
    report: Report,
    json: bool,
    report_stream: ReportStream,
}

impl CommandOutcome {
    /// Append warnings to an outcome whose report is already built, so a caller
    /// can attach them once for every exit path instead of at each `return`.
    pub(crate) fn with_warnings(
        mut self,
        warnings: impl IntoIterator<Item = ReportDiagnostic>,
    ) -> Self {
        self.report.warnings.extend(warnings);
        self
    }

    #[cfg(test)]
    pub(crate) fn warning_codes(&self) -> Vec<ReportCode> {
        self.report
            .warnings
            .iter()
            .map(|warning| warning.code)
            .collect()
    }
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

pub(crate) fn slow_file_skipped_diagnostic(error: &DiscoveryError) -> ReportDiagnostic {
    diagnostic(
        ReportCode::SlowFileSkipped,
        error.message.clone(),
        Some(error.path.clone()),
        Some(error.root_relative_path.clone()),
        true,
        json!({}),
    )
}

/// One recoverable warning per file whose extraction passes disagreed about a
/// shared reference site, plus a trailing summary when the writer's sample
/// bound dropped files. The write already committed: the first site row won and
/// per-row attribution is intact, so this reports an extractor bug rather than a
/// failure.
pub(crate) fn reference_site_conflict_diagnostics(
    conflicts: &ReferenceSiteConflicts,
    root: Option<&Path>,
) -> Vec<ReportDiagnostic> {
    if conflicts.total == 0 {
        return Vec::new();
    }

    let mut diagnostics: Vec<ReportDiagnostic> = conflicts
        .files
        .iter()
        .map(|file| {
            diagnostic(
                ReportCode::ReferenceSitePayloadConflict,
                format!(
                    "extraction passes disagreed about {} shared reference site payload(s); \
                     first write kept",
                    file.conflicts
                ),
                root.map(|root| root.join(&file.path).display().to_string()),
                Some(file.path.clone()),
                true,
                json!({
                    "language": file.language,
                    "conflict_count": file.conflicts,
                    "sites": file
                        .sites
                        .iter()
                        .map(|site| json!({
                            "reference_site_id": site.reference_site_id,
                            "fields": site.fields,
                        }))
                        .collect::<Vec<_>>(),
                }),
            )
        })
        .collect();

    if conflicts.files_affected > conflicts.files.len() {
        diagnostics.push(diagnostic(
            ReportCode::ReferenceSitePayloadConflict,
            format!(
                "{} file(s) had reference-site payload conflicts; {} reported",
                conflicts.files_affected,
                conflicts.files.len()
            ),
            None,
            None,
            true,
            json!({
                "conflict_count": conflicts.total,
                "files_affected": conflicts.files_affected,
                "files_reported": conflicts.files.len(),
            }),
        ));
    }

    diagnostics
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

    let rendered = human_report(&outcome.report);
    if outcome.exit_code == 0 {
        let _ = write!(io::stdout(), "{rendered}");
    } else {
        let _ = write!(io::stderr(), "{rendered}");
    }
}

fn write_json(mut writer: impl Write, report: &Report) {
    let _ = serde_json::to_writer(&mut writer, report);
    let _ = writeln!(writer);
}

fn human_report(report: &Report) -> String {
    let mut rendered = format!("{}\n", human_status(report));
    for error in &report.errors {
        rendered.push_str(&human_diagnostic(error));
    }
    if report.status != ReportStatus::Ok {
        for warning in &report.warnings {
            rendered.push_str(&human_diagnostic(warning));
        }
    }
    if let Some(counts) = human_file_counts(report) {
        rendered.push_str(&counts);
    }
    rendered
}

fn human_diagnostic(diagnostic: &ReportDiagnostic) -> String {
    let code = diagnostic.code.as_str();
    let message = &diagnostic.message;
    match diagnostic
        .path
        .as_deref()
        .or(diagnostic.root_relative_path.as_deref())
    {
        Some(path) => format!("{code}: {message} ({path})\n"),
        None => format!("{code}: {message}\n"),
    }
}

fn human_file_counts(report: &Report) -> Option<String> {
    matches!(
        report.operation,
        ReportOperation::Scan | ReportOperation::Update | ReportOperation::Delete
    )
    .then(|| {
        format!(
            "files: scanned={} changed={} unchanged={} failed={}\n",
            report.counts.files_scanned,
            report.counts.files_changed,
            report.counts.files_unchanged,
            report.counts.files_failed
        )
    })
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

/// Build the `reference_resolution` scan-report section from the captured
/// per-pass [`ResolutionReport`] (per-language/per-tier/per-outcome counts,
/// durable status, gated languages) plus the writer's resolution outcome (the
/// in-transaction row counts and any non-fatal failure message).
///
/// Returns `None` when neither a report nor a failure exists (a hookless write),
/// so every other command's report shape is unchanged. When present it is attached
/// under the report's `languages` section — the only additive value slot the
/// committed `Report` struct exposes — namespaced under `reference_resolution`.
pub(crate) fn resolution_report_section(
    report: Option<&ResolutionReport>,
    write_result: &WriteResult,
) -> Option<Value> {
    let failed = write_result.resolution.failed.clone();
    if report.is_none() && failed.is_none() {
        return None;
    }
    let counts = &write_result.resolution.counts;
    let (status, version, last_full_revision, gated, by_language, totals, origin_totals) =
        match report {
            Some(report) => (
                report.status.as_str().to_string(),
                report.version,
                report.last_full_revision,
                report
                    .tier2_gated_languages
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                report
                    .rows
                    .iter()
                    .map(|row| {
                        json!({
                            "language": row.language,
                            "origin": row.origin,
                            "raw_kind": row.raw_kind,
                            "canonical_kind": row.canonical_kind,
                            "tier": row.tier,
                            "method": row.method,
                            "outcome": row.outcome,
                            "span_present": row.span_present,
                            "count": row.count,
                        })
                    })
                    .collect::<Vec<_>>(),
                resolution_totals(&report.rows, None),
                report
                    .rows
                    .iter()
                    .map(|row| row.origin.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .map(|origin| {
                        (
                            origin.to_string(),
                            resolution_totals(&report.rows, Some(origin)),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>(),
            ),
            None => (
                "failed".to_string(),
                0,
                0,
                Vec::new(),
                Vec::new(),
                json!({
                    "total": 0,
                    "attempted": 0,
                    "resolved": 0,
                    "ambiguous": 0,
                    "missing": 0,
                    "no_context": 0,
                    "unresolved_pending": 0,
                    "unattempted": 0,
                    "span_present": 0,
                    "span_missing": 0,
                }),
                serde_json::Map::new(),
            ),
        };
    Some(json!({
        "reference_resolution": {
            "status": status,
            "version": version,
            "last_full_revision": last_full_revision,
            "counts": {
                "pending_resolutions": counts.pending_resolutions,
                "identifier_resolutions": counts.identifier_resolutions,
            },
            "totals": totals,
            "origin_totals": origin_totals,
            "gated_languages": gated,
            "failed": failed,
            "by_language": by_language,
        }
    }))
}

fn resolution_totals(
    rows: &[julie_extract_artifact::resolution_store::ResolutionReportRow],
    origin: Option<&str>,
) -> Value {
    let matching = |row: &&julie_extract_artifact::resolution_store::ResolutionReportRow| {
        origin.is_none_or(|expected| row.origin == expected)
    };
    let count = |outcome: &str| {
        rows.iter()
            .filter(matching)
            .filter(|row| row.outcome == outcome)
            .map(|row| row.count)
            .sum::<i64>()
    };
    let total = rows
        .iter()
        .filter(matching)
        .map(|row| row.count)
        .sum::<i64>();
    let no_context = count("no_context");
    let unattempted = count("unattempted");
    json!({
        "total": total,
        "attempted": total - no_context - unattempted,
        "resolved": count("resolved"),
        "ambiguous": count("ambiguous"),
        "missing": count("missing"),
        "no_context": no_context,
        "unresolved_pending": count("unresolved_pending"),
        "unattempted": unattempted,
        "span_present": rows.iter().filter(matching).filter(|row| row.span_present).map(|row| row.count).sum::<i64>(),
        "span_missing": rows.iter().filter(matching).filter(|row| !row.span_present).map(|row| row.count).sum::<i64>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use julie_extract_artifact::model::{ReferenceSiteConflictFile, ReferenceSiteConflictSite};

    fn report_for(status: ReportStatus, operation: ReportOperation) -> Report {
        base_report(
            status,
            operation,
            ReportMode::Incremental,
            ReportInput {
                db_path: None,
                root_path: None,
                file_path: None,
                root_relative_path: None,
                format: None,
                output_path: None,
            },
        )
    }

    fn conflict_file(root_relative_path: &str, conflicts: i64) -> ReferenceSiteConflictFile {
        ReferenceSiteConflictFile {
            path: root_relative_path.to_string(),
            language: "powershell".to_string(),
            conflicts,
            sites: vec![ReferenceSiteConflictSite {
                reference_site_id: "site-1".to_string(),
                fields: vec!["containing_symbol_id"],
            }],
        }
    }

    #[test]
    fn reference_site_conflicts_become_one_recoverable_warning_per_file() {
        let conflicts = ReferenceSiteConflicts {
            total: 3,
            files_affected: 1,
            files: vec![conflict_file("scripts/install.ps1", 3)],
        };

        let diagnostics =
            reference_site_conflict_diagnostics(&conflicts, Some(Path::new("/repo")));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            ReportCode::ReferenceSitePayloadConflict
        );
        assert!(diagnostics[0].recoverable);
        assert_eq!(
            diagnostics[0].path.as_deref(),
            Some("/repo/scripts/install.ps1")
        );
        assert_eq!(
            diagnostics[0].root_relative_path.as_deref(),
            Some("scripts/install.ps1")
        );
        assert_eq!(diagnostics[0].details["conflict_count"], 3);
        assert_eq!(
            diagnostics[0].details["sites"][0]["fields"][0],
            "containing_symbol_id"
        );
    }

    #[test]
    fn reference_site_conflict_sample_bound_reports_the_unlisted_files() {
        let conflicts = ReferenceSiteConflicts {
            total: 9,
            files_affected: 40,
            files: vec![conflict_file("a.ps1", 1)],
        };

        let diagnostics =
            reference_site_conflict_diagnostics(&conflicts, Some(Path::new("/repo")));

        assert_eq!(diagnostics.len(), 2);
        let summary = diagnostics.last().unwrap();
        assert_eq!(summary.path, None);
        assert_eq!(summary.details["files_affected"], 40);
        assert_eq!(summary.details["files_reported"], 1);
        assert_eq!(summary.details["conflict_count"], 9);
    }

    #[test]
    fn agreeing_passes_emit_no_reference_site_warning() {
        assert!(
            reference_site_conflict_diagnostics(
                &ReferenceSiteConflicts::default(),
                Some(Path::new("/repo"))
            )
            .is_empty()
        );
    }

    fn read_failure() -> ReportDiagnostic {
        diagnostic(
            ReportCode::ReadFailed,
            "permission denied",
            Some("/repo/src/lib.rs".to_string()),
            Some("src/lib.rs".to_string()),
            true,
            json!({}),
        )
    }

    fn write_failure() -> ReportDiagnostic {
        diagnostic(
            ReportCode::DbWriteFailed,
            "SQLite artifact write failed: disk I/O error",
            None,
            None,
            false,
            json!({}),
        )
    }

    fn skipped_warning() -> ReportDiagnostic {
        diagnostic(
            ReportCode::SlowFileSkipped,
            "file exceeded the read budget",
            None,
            Some("vendor/huge.rs".to_string()),
            true,
            json!({}),
        )
    }

    #[test]
    fn failed_report_renders_status_diagnostics_and_file_counts() {
        let mut report = report_for(ReportStatus::Failed, ReportOperation::Scan)
            .with_error(read_failure())
            .with_error(write_failure())
            .with_warning(skipped_warning());
        report.counts.files_scanned = 12;
        report.counts.files_changed = 3;
        report.counts.files_unchanged = 8;
        report.counts.files_failed = 1;

        assert_eq!(
            human_report(&report),
            "failed\n\
             read_failed: permission denied (/repo/src/lib.rs)\n\
             db_write_failed: SQLite artifact write failed: disk I/O error\n\
             slow_file_skipped: file exceeded the read budget (vendor/huge.rs)\n\
             files: scanned=12 changed=3 unchanged=8 failed=1\n"
        );
    }

    #[test]
    fn successful_report_renders_counts_without_warning_noise() {
        let mut report =
            report_for(ReportStatus::Ok, ReportOperation::Update).with_warning(skipped_warning());
        report.counts.files_scanned = 1;
        report.counts.files_changed = 1;

        assert_eq!(
            human_report(&report),
            "ok\nfiles: scanned=1 changed=1 unchanged=0 failed=0\n"
        );
    }

    #[test]
    fn reports_without_file_work_render_only_the_status_and_diagnostics() {
        let report =
            report_for(ReportStatus::Failed, ReportOperation::Export).with_error(diagnostic(
                ReportCode::UnsupportedFormat,
                "export format is not supported",
                None,
                None,
                false,
                json!({}),
            ));

        assert_eq!(
            human_report(&report),
            "failed\nunsupported_format: export format is not supported\n"
        );
    }
}
