use serde::{Deserialize, Serialize, ser::SerializeStruct};

use crate::model::RowCounts;

pub const REPORT_SCHEMA_VERSION: i64 = 1;

pub const SQLITE_ROW_DOMAINS: &[&str] = &[
    "artifact_metadata",
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
    "extraction_revisions",
    "revision_file_changes",
    "files",
    "symbols",
    "symbol_annotations",
    "identifiers",
    "relationships",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "parse_diagnostics",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub status: ReportStatus,
    pub operation: ReportOperation,
    pub mode: ReportMode,
    pub input: ReportInput,
    pub artifact: Option<ArtifactReport>,
    pub tool: ToolReport,
    pub revision: Option<ReportRevision>,
    pub counts: ReportCounts,
    pub errors: Vec<ReportDiagnostic>,
    pub warnings: Vec<ReportDiagnostic>,
    pub languages: Option<serde_json::Value>,
}

impl Serialize for Report {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let field_count = if self.languages.is_some() { 12 } else { 11 };
        let mut state = serializer.serialize_struct("Report", field_count)?;
        state.serialize_field("report_schema_version", &REPORT_SCHEMA_VERSION)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("operation", &self.operation)?;
        state.serialize_field("mode", &self.mode)?;
        state.serialize_field("input", &self.input)?;
        state.serialize_field("artifact", &self.artifact)?;
        state.serialize_field("tool", &self.tool)?;
        state.serialize_field("revision", &self.revision)?;
        state.serialize_field("counts", &self.counts)?;
        state.serialize_field("errors", &self.errors)?;
        state.serialize_field("warnings", &self.warnings)?;
        if let Some(languages) = &self.languages {
            state.serialize_field("languages", languages)?;
        }
        state.end()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Ok,
    NoChange,
    Unsupported,
    NotFound,
    Partial,
    Failed,
}

impl ReportStatus {
    pub const ALL: [Self; 6] = [
        Self::Ok,
        Self::NoChange,
        Self::Unsupported,
        Self::NotFound,
        Self::Partial,
        Self::Failed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOperation {
    Scan,
    Update,
    Delete,
    Info,
    Export,
    Languages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportMode {
    Incremental,
    Force,
    SingleFile,
    ReadOnly,
    Jsonl,
    CapabilitySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportInput {
    pub db_path: Option<String>,
    pub root_path: Option<String>,
    pub file_path: Option<String>,
    pub root_relative_path: Option<String>,
    pub format: Option<String>,
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReport {
    pub db_path: String,
    pub root_path: String,
    pub artifact_id: String,
    pub schema_version: i64,
    pub extract_contract_version: i64,
    pub sqlite_schema_version: i64,
    pub jsonl_schema_version: Option<i64>,
    pub hash_algorithm: String,
    pub parser_inventory_fingerprint: String,
    pub capability_snapshot_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReport {
    pub binary_name: String,
    pub binary_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRevision {
    pub latest_revision_id: Option<i64>,
    pub created_revision_id: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCounts {
    pub files_scanned: i64,
    pub files_changed: i64,
    pub files_unchanged: i64,
    pub files_unsupported: i64,
    pub files_deleted: i64,
    pub files_failed: i64,
    pub rows_written: RowDomainCounts,
    pub totals: RowDomainCounts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowDomainCounts {
    pub artifact_metadata: i64,
    pub parser_inventory: i64,
    pub language_capabilities: i64,
    pub language_capability_fixtures: i64,
    pub language_capability_gaps: i64,
    pub extraction_revisions: i64,
    pub revision_file_changes: i64,
    pub files: i64,
    pub symbols: i64,
    pub symbol_annotations: i64,
    pub identifiers: i64,
    pub relationships: i64,
    pub pending_relationships: i64,
    pub type_facts: i64,
    pub type_argument_usages: i64,
    pub type_arguments: i64,
    pub literals: i64,
    pub parse_diagnostics: i64,
}

impl RowDomainCounts {
    pub fn from_extraction_rows(row_counts: &RowCounts) -> Self {
        Self {
            revision_file_changes: row_counts.revision_file_changes,
            files: row_counts.files,
            symbols: row_counts.symbols,
            symbol_annotations: row_counts.symbol_annotations,
            identifiers: row_counts.identifiers,
            relationships: row_counts.relationships,
            pending_relationships: row_counts.pending_relationships,
            type_facts: row_counts.type_facts,
            type_argument_usages: row_counts.type_argument_usages,
            type_arguments: row_counts.type_arguments,
            literals: row_counts.literals,
            parse_diagnostics: row_counts.parse_diagnostics,
            ..Self::default()
        }
    }
}

impl From<&RowCounts> for RowDomainCounts {
    fn from(value: &RowCounts) -> Self {
        Self::from_extraction_rows(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportCode {
    UsageError,
    InvalidPath,
    FileOutsideRoot,
    FileNotFound,
    RootMismatch,
    SchemaMigrationRequired,
    SchemaIncompatible,
    ContractIncompatible,
    DbOpenFailed,
    DbWriteFailed,
    LockTimeout,
    UnsupportedFormat,
    UnsupportedFile,
    ReadFailed,
    ParseFailed,
    DataLossGuard,
    ExportFailed,
    InternalError,
    MetadataMissing,
    CapabilityGap,
    SlowFileSkipped,
}

impl ReportCode {
    pub const ERROR_CODES: [Self; 18] = [
        Self::UsageError,
        Self::InvalidPath,
        Self::FileOutsideRoot,
        Self::FileNotFound,
        Self::RootMismatch,
        Self::SchemaMigrationRequired,
        Self::SchemaIncompatible,
        Self::ContractIncompatible,
        Self::DbOpenFailed,
        Self::DbWriteFailed,
        Self::LockTimeout,
        Self::UnsupportedFormat,
        Self::UnsupportedFile,
        Self::ReadFailed,
        Self::ParseFailed,
        Self::DataLossGuard,
        Self::ExportFailed,
        Self::InternalError,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportDiagnostic {
    pub code: ReportCode,
    pub message: String,
    pub path: Option<String>,
    pub root_relative_path: Option<String>,
    pub recoverable: bool,
    pub details: serde_json::Value,
}
