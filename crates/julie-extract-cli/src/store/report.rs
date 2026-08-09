use std::io::{self, Write};

use serde::{Deserialize, Serialize};

pub const STORE_REPORT_SCHEMA_VERSION: i64 = 1;
pub const STORE_EXIT_SUCCESS: u8 = 0;
pub const STORE_EXIT_OPERATIONAL_FAILURE: u8 = 1;
pub const STORE_EXIT_USAGE: u8 = 2;
pub const STORE_EXIT_INCOMPATIBLE: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreOperation {
    Import,
    Update,
    Delete,
    Resolve,
    Export,
    FromArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRequestedLevel {
    L1,
    Full,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreManifestDisposition {
    #[default]
    NotPublished,
    Created,
    Reused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreCoordinatorDisposition {
    NotStarted,
    Queued,
    Claimed,
    Committed,
    Acknowledged,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreLevelCompletion {
    pub l1: bool,
    pub l2: bool,
    pub l3: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreManifestReport {
    pub generation: Option<u64>,
    pub hash: Option<String>,
    pub disposition: StoreManifestDisposition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRowCounts {
    pub file_versions: u64,
    pub l1: u64,
    pub l2: u64,
    pub l3: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreResolutionState {
    Unbound,
    Converging,
    Exact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreResolutionReport {
    pub state: StoreResolutionState,
    pub exact_at_matches: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_at_generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_lower_bound: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_gap_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_gap_files: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreExportDisposition {
    Created,
    Reused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreExportReport {
    pub output: String,
    pub disposition: StoreExportDisposition,
}

impl Default for StoreResolutionReport {
    fn default() -> Self {
        Self {
            state: StoreResolutionState::Unbound,
            exact_at_matches: false,
            base_id: None,
            delta_generation: None,
            exact_at_generation: None,
            gap_lower_bound: None,
            exact_gap_rows: None,
            exact_gap_files: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRequestState {
    Queued,
    Claimed,
    Committed,
    Acknowledged,
    Failed,
}

impl StoreRequestState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Committed => "committed",
            Self::Acknowledged => "acknowledged",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreFailureClass {
    None,
    InvalidArguments,
    InvalidPath,
    InvalidIdentifier,
    FamilyMismatch,
    StoreNotFound,
    StoreIncompatible,
    ViewNotFound,
    ViewRootMismatch,
    L1ProjectionMismatch,
    ChangedBetweenWaves,
    IdempotencyConflict,
    RequestTimeout,
    Busy,
    ResolutionInputIncomplete,
    ResolutionFailed,
    ResolutionNotExact,
    OutputIdentityMismatch,
    CapacityInsufficient,
    Internal,
}

impl StoreFailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InvalidArguments => "invalid_arguments",
            Self::InvalidPath => "invalid_path",
            Self::InvalidIdentifier => "invalid_identifier",
            Self::FamilyMismatch => "family_mismatch",
            Self::StoreNotFound => "store_not_found",
            Self::StoreIncompatible => "store_incompatible",
            Self::ViewNotFound => "view_not_found",
            Self::ViewRootMismatch => "view_root_mismatch",
            Self::L1ProjectionMismatch => "l1_projection_mismatch",
            Self::ChangedBetweenWaves => "changed_between_waves",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::RequestTimeout => "request_timeout",
            Self::Busy => "busy",
            Self::ResolutionInputIncomplete => "resolution_input_incomplete",
            Self::ResolutionFailed => "resolution_failed",
            Self::ResolutionNotExact => "resolution_not_exact",
            Self::OutputIdentityMismatch => "output_identity_mismatch",
            Self::CapacityInsufficient => "capacity_insufficient",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRequestReport {
    pub id: String,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreErrorReport {
    pub class: StoreFailureClass,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreReport {
    pub report_schema_version: i64,
    pub operation: StoreOperation,
    pub request: StoreRequestReport,
    pub family_id: String,
    pub view_id: String,
    pub root: String,
    pub state: StoreRequestState,
    pub requested_level: StoreRequestedLevel,
    pub completion: StoreLevelCompletion,
    pub manifest: StoreManifestReport,
    pub row_counts: StoreRowCounts,
    pub resolution: StoreResolutionReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<StoreExportReport>,
    pub coordinator: StoreCoordinatorDisposition,
    pub failure_class: StoreFailureClass,
    pub error: Option<StoreErrorReport>,
}

impl StoreReport {
    pub fn new(
        request_id: impl Into<String>,
        family_id: impl Into<String>,
        view_id: impl Into<String>,
        state: StoreRequestState,
    ) -> Self {
        let failed = state == StoreRequestState::Failed;
        Self {
            report_schema_version: STORE_REPORT_SCHEMA_VERSION,
            operation: StoreOperation::Import,
            request: StoreRequestReport {
                id: request_id.into(),
                idempotency_key: None,
            },
            family_id: family_id.into(),
            view_id: view_id.into(),
            root: String::new(),
            state,
            requested_level: StoreRequestedLevel::Full,
            completion: StoreLevelCompletion::default(),
            manifest: StoreManifestReport::default(),
            row_counts: StoreRowCounts::default(),
            resolution: StoreResolutionReport::default(),
            export: None,
            coordinator: match state {
                StoreRequestState::Queued => StoreCoordinatorDisposition::Queued,
                StoreRequestState::Claimed => StoreCoordinatorDisposition::Claimed,
                StoreRequestState::Committed => StoreCoordinatorDisposition::Committed,
                StoreRequestState::Acknowledged => StoreCoordinatorDisposition::Acknowledged,
                StoreRequestState::Failed => StoreCoordinatorDisposition::Failed,
            },
            failure_class: if failed {
                StoreFailureClass::Internal
            } else {
                StoreFailureClass::None
            },
            error: failed.then(|| StoreErrorReport {
                class: StoreFailureClass::Internal,
                message: "store operation failed without a failure class".to_string(),
            }),
        }
    }

    pub fn with_root(mut self, root: impl Into<String>) -> Self {
        self.root = root.into();
        self
    }

    pub fn with_operation(mut self, operation: StoreOperation) -> Self {
        self.operation = operation;
        self
    }

    pub fn with_requested_level(mut self, level: StoreRequestedLevel) -> Self {
        self.requested_level = level;
        self
    }

    pub fn with_completion(mut self, completion: StoreLevelCompletion) -> Self {
        self.completion = completion;
        self
    }

    pub fn with_manifest(mut self, manifest: StoreManifestReport) -> Self {
        self.manifest = manifest;
        self
    }

    pub fn with_row_counts(mut self, row_counts: StoreRowCounts) -> Self {
        self.row_counts = row_counts;
        self
    }

    pub fn with_coordinator(mut self, coordinator: StoreCoordinatorDisposition) -> Self {
        self.coordinator = coordinator;
        self
    }

    pub fn with_export(mut self, export: StoreExportReport) -> Self {
        self.export = Some(export);
        self
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.request.idempotency_key = Some(key.into());
        self
    }

    pub fn with_failure(mut self, class: StoreFailureClass, message: impl Into<String>) -> Self {
        self.state = StoreRequestState::Failed;
        self.coordinator = StoreCoordinatorDisposition::Failed;
        let class = if class == StoreFailureClass::None {
            StoreFailureClass::Internal
        } else {
            class
        };
        self.failure_class = class;
        self.error = Some(StoreErrorReport {
            class,
            message: message.into(),
        });
        self
    }

    pub fn is_failed(&self) -> bool {
        self.state == StoreRequestState::Failed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutputFormat {
    Json,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreOutputPlan {
    pub format: StoreOutputFormat,
    pub stream: StoreOutputStream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCommandOutcome {
    report: StoreReport,
    exit_code: u8,
}

impl StoreCommandOutcome {
    fn new(report: StoreReport, exit_code: u8) -> Self {
        Self { report, exit_code }
    }

    pub fn queued(report: StoreReport) -> Self {
        if report.state == StoreRequestState::Failed
            || report.failure_class != StoreFailureClass::None
            || report.error.is_some()
        {
            return Self::failed(report);
        }
        Self::new(report, STORE_EXIT_SUCCESS)
    }

    pub fn failed(report: StoreReport) -> Self {
        let report = normalize_failed_report(report);
        Self::new(report, STORE_EXIT_OPERATIONAL_FAILURE)
    }

    pub fn observed_incomplete(report: StoreReport) -> Self {
        Self::new(report, STORE_EXIT_OPERATIONAL_FAILURE)
    }

    pub fn usage(report: StoreReport) -> Self {
        Self::new(normalize_usage_report(report), STORE_EXIT_USAGE)
    }

    pub fn incompatible(mut report: StoreReport) -> Self {
        report.state = StoreRequestState::Failed;
        report.coordinator = StoreCoordinatorDisposition::Failed;
        report.failure_class = StoreFailureClass::StoreIncompatible;
        report.error = Some(StoreErrorReport {
            class: StoreFailureClass::StoreIncompatible,
            message: "store format is incompatible".to_string(),
        });
        Self::new(report, STORE_EXIT_INCOMPATIBLE)
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn report(&self) -> &StoreReport {
        &self.report
    }

    pub fn render_json(&self) -> String {
        let mut output = serde_json::to_string(&self.report)
            .expect("store reports contain only serializable fields");
        output.push('\n');
        output
    }

    pub fn render(&self, format: StoreOutputFormat) -> String {
        match format {
            StoreOutputFormat::Json => self.render_json(),
            StoreOutputFormat::Human => self.render_human(),
        }
    }

    pub fn render_human(&self) -> String {
        let mut output = String::new();
        output.push_str(self.report.state.as_str());
        output.push('\n');
        output.push_str("operation: ");
        output.push_str(match self.report.operation {
            StoreOperation::Import => "import",
            StoreOperation::Update => "update",
            StoreOperation::Delete => "delete",
            StoreOperation::Resolve => "resolve",
            StoreOperation::Export => "export",
            StoreOperation::FromArtifact => "from_artifact",
        });
        output.push('\n');
        output.push_str("request: ");
        output.push_str(&self.report.request.id);
        output.push('\n');
        output.push_str("idempotency_key: ");
        output.push_str(
            self.report
                .request
                .idempotency_key
                .as_deref()
                .unwrap_or("none"),
        );
        output.push('\n');
        output.push_str("family: ");
        output.push_str(&self.report.family_id);
        output.push('\n');
        output.push_str("view: ");
        output.push_str(&self.report.view_id);
        output.push('\n');
        output.push_str("root: ");
        output.push_str(&self.report.root);
        output.push('\n');
        output.push_str("resolution: state=");
        output.push_str(match self.report.resolution.state {
            StoreResolutionState::Unbound => "unbound",
            StoreResolutionState::Converging => "converging",
            StoreResolutionState::Exact => "exact",
        });
        output.push_str(" exact_at_matches=");
        output.push_str(if self.report.resolution.exact_at_matches {
            "true"
        } else {
            "false"
        });
        output.push('\n');
        if self.report.operation == StoreOperation::Resolve {
            output.push_str("resolution_detail: base=");
            output.push_str(self.report.resolution.base_id.as_deref().unwrap_or("none"));
            output.push_str(" delta_generation=");
            push_optional_u64(&mut output, self.report.resolution.delta_generation);
            output.push_str(" exact_at_generation=");
            push_optional_u64(&mut output, self.report.resolution.exact_at_generation);
            output.push_str(" gap_lower_bound=");
            push_optional_u64(&mut output, self.report.resolution.gap_lower_bound);
            output.push_str(" exact_gap_rows=");
            push_optional_u64(&mut output, self.report.resolution.exact_gap_rows);
            output.push_str(" exact_gap_files=");
            push_optional_u64(&mut output, self.report.resolution.exact_gap_files);
            output.push('\n');
        }
        if let Some(export) = &self.report.export {
            output.push_str("export: output=");
            output.push_str(&export.output);
            output.push_str(" disposition=");
            output.push_str(match export.disposition {
                StoreExportDisposition::Created => "created",
                StoreExportDisposition::Reused => "reused",
            });
            output.push('\n');
        }
        output.push_str("state: ");
        output.push_str(self.report.state.as_str());
        output.push('\n');
        output.push_str("requested_level: ");
        output.push_str(match self.report.requested_level {
            StoreRequestedLevel::L1 => "l1",
            StoreRequestedLevel::Full => "full",
            StoreRequestedLevel::NotApplicable => "not_applicable",
        });
        output.push('\n');
        output.push_str("completion: ");
        output.push_str(if self.report.completion.l1 { "l1" } else { "-" });
        output.push(' ');
        output.push_str(if self.report.completion.l2 { "l2" } else { "-" });
        output.push(' ');
        output.push_str(if self.report.completion.l3 { "l3" } else { "-" });
        output.push('\n');
        output.push_str("manifest: generation=");
        output.push_str(
            &self
                .report
                .manifest
                .generation
                .map_or_else(|| "none".to_string(), |generation| generation.to_string()),
        );
        output.push_str(" hash=");
        output.push_str(self.report.manifest.hash.as_deref().unwrap_or("none"));
        output.push_str(" disposition=");
        output.push_str(match self.report.manifest.disposition {
            StoreManifestDisposition::NotPublished => "not_published",
            StoreManifestDisposition::Created => "created",
            StoreManifestDisposition::Reused => "reused",
        });
        output.push('\n');
        output.push_str("rows: file_versions=");
        output.push_str(&self.report.row_counts.file_versions.to_string());
        output.push_str(" l1=");
        output.push_str(&self.report.row_counts.l1.to_string());
        output.push_str(" l2=");
        output.push_str(&self.report.row_counts.l2.to_string());
        output.push_str(" l3=");
        output.push_str(&self.report.row_counts.l3.to_string());
        output.push('\n');
        output.push_str("coordinator: ");
        output.push_str(match self.report.coordinator {
            StoreCoordinatorDisposition::NotStarted => "not_started",
            StoreCoordinatorDisposition::Queued => "queued",
            StoreCoordinatorDisposition::Claimed => "claimed",
            StoreCoordinatorDisposition::Committed => "committed",
            StoreCoordinatorDisposition::Acknowledged => "acknowledged",
            StoreCoordinatorDisposition::Failed => "failed",
        });
        output.push('\n');
        output.push_str("failure_class: ");
        output.push_str(self.report.failure_class.as_str());
        output.push('\n');
        if let Some(error) = &self.report.error {
            output.push_str("error: ");
            output.push_str(error.class.as_str());
            output.push_str(": ");
            output.push_str(&error.message);
            output.push('\n');
        }
        output
    }

    pub fn output_plan(&self, json: bool) -> StoreOutputPlan {
        StoreOutputPlan {
            format: if json {
                StoreOutputFormat::Json
            } else {
                StoreOutputFormat::Human
            },
            stream: if json || self.exit_code == STORE_EXIT_SUCCESS {
                StoreOutputStream::Stdout
            } else {
                StoreOutputStream::Stderr
            },
        }
    }

    pub fn write_human<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(self.render_human().as_bytes())
    }

    pub fn write_json<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(self.render_json().as_bytes())
    }
}

fn push_optional_u64(output: &mut String, value: Option<u64>) {
    match value {
        Some(value) => output.push_str(&value.to_string()),
        None => output.push_str("none"),
    }
}

fn normalize_failed_report(mut report: StoreReport) -> StoreReport {
    report.state = StoreRequestState::Failed;
    report.coordinator = StoreCoordinatorDisposition::Failed;
    if report.failure_class == StoreFailureClass::None {
        report.failure_class = StoreFailureClass::Internal;
    }
    match report.error.as_mut() {
        Some(error) => error.class = report.failure_class,
        None => {
            report.error = Some(StoreErrorReport {
                class: report.failure_class,
                message: "store operation failed without a failure message".to_string(),
            });
        }
    }
    report
}

fn normalize_usage_report(mut report: StoreReport) -> StoreReport {
    report.state = StoreRequestState::Failed;
    report.coordinator = StoreCoordinatorDisposition::Failed;
    if report.failure_class == StoreFailureClass::None {
        report.failure_class = StoreFailureClass::InvalidArguments;
    }
    match report.error.as_mut() {
        Some(error) => error.class = report.failure_class,
        None => {
            report.error = Some(StoreErrorReport {
                class: report.failure_class,
                message: "invalid store request".to_string(),
            });
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::{StoreFailureClass, StoreReport, StoreRequestState};

    #[test]
    fn success_has_no_failure_class() {
        let report = StoreReport::new("request", "family", "view", StoreRequestState::Queued);
        assert_eq!(report.failure_class, StoreFailureClass::None);
        assert!(report.error.is_none());
    }
}
