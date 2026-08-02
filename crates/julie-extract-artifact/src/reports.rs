use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, ser::SerializeStruct};

use crate::model::RowCounts;

pub const REPORT_SCHEMA_VERSION: i64 = 3;

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
    "reference_sites",
    "identifiers",
    "relationships",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "source_regions",
    "structural_facts",
    "complexity_metrics",
    "parse_diagnostics",
    "pending_resolutions",
    "identifier_resolutions",
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
    pub profile: Option<ReportProfile>,
    pub errors: Vec<ReportDiagnostic>,
    pub warnings: Vec<ReportDiagnostic>,
    pub languages: Option<serde_json::Value>,
    /// Additive `languages` report section: the structural-fact pattern
    /// registry payload. Present only when the command populates it (the
    /// `languages` command); serialized as the top-level `structural_fact_patterns`
    /// key when `Some`, and omitted entirely when `None` so every other command's
    /// report shape is byte-unchanged.
    pub structural_fact_patterns: Option<serde_json::Value>,
}

impl Serialize for Report {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let field_count = 11
            + usize::from(self.profile.is_some())
            + usize::from(self.languages.is_some())
            + usize::from(self.structural_fact_patterns.is_some());
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
        if let Some(profile) = &self.profile {
            state.serialize_field("profile", profile)?;
        }
        state.serialize_field("errors", &self.errors)?;
        state.serialize_field("warnings", &self.warnings)?;
        if let Some(languages) = &self.languages {
            state.serialize_field("languages", languages)?;
        }
        if let Some(structural_fact_patterns) = &self.structural_fact_patterns {
            state.serialize_field("structural_fact_patterns", structural_fact_patterns)?;
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
pub struct ReportProfile {
    pub total_duration_ms: u64,
    pub phases: BTreeMap<String, u64>,
    pub languages: BTreeMap<String, ReportLanguageProfile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportLanguageProfile {
    pub files: i64,
    pub changed_files: i64,
    pub unchanged_files: i64,
    pub failed_files: i64,
    pub bytes: i64,
    pub read_duration_ms: u64,
    pub extract_duration_ms: u64,
    pub spool_write_duration_ms: u64,
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
    pub file_rows_truncated: bool,
    pub file_rows: Vec<ReportFileRows>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportFileRows {
    pub path: String,
    pub language: String,
    pub status: String,
    pub total_rows: i64,
    pub rows: RowDomainCounts,
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
    pub reference_sites: i64,
    pub identifiers: i64,
    pub relationships: i64,
    pub pending_relationships: i64,
    pub type_facts: i64,
    pub type_argument_usages: i64,
    pub type_arguments: i64,
    pub literals: i64,
    pub source_regions: i64,
    pub structural_facts: i64,
    pub complexity_metrics: i64,
    pub parse_diagnostics: i64,
    /// Resolution overlay (schema v4). Written only by the writer's resolution
    /// hook, never by the extraction row-count path, so revision accounting stays
    /// truthful about the two derived overlay tables.
    pub pending_resolutions: i64,
    pub identifier_resolutions: i64,
}

impl RowDomainCounts {
    pub fn has_rows(&self) -> bool {
        self.artifact_metadata != 0
            || self.parser_inventory != 0
            || self.language_capabilities != 0
            || self.language_capability_fixtures != 0
            || self.language_capability_gaps != 0
            || self.extraction_revisions != 0
            || self.revision_file_changes != 0
            || self.files != 0
            || self.symbols != 0
            || self.symbol_annotations != 0
            || self.reference_sites != 0
            || self.identifiers != 0
            || self.relationships != 0
            || self.pending_relationships != 0
            || self.type_facts != 0
            || self.type_argument_usages != 0
            || self.type_arguments != 0
            || self.literals != 0
            || self.source_regions != 0
            || self.structural_facts != 0
            || self.complexity_metrics != 0
            || self.parse_diagnostics != 0
            || self.pending_resolutions != 0
            || self.identifier_resolutions != 0
    }

    pub fn add_counts(&mut self, other: &Self) {
        self.artifact_metadata += other.artifact_metadata;
        self.parser_inventory += other.parser_inventory;
        self.language_capabilities += other.language_capabilities;
        self.language_capability_fixtures += other.language_capability_fixtures;
        self.language_capability_gaps += other.language_capability_gaps;
        self.extraction_revisions += other.extraction_revisions;
        self.revision_file_changes += other.revision_file_changes;
        self.files += other.files;
        self.symbols += other.symbols;
        self.symbol_annotations += other.symbol_annotations;
        self.reference_sites += other.reference_sites;
        self.identifiers += other.identifiers;
        self.relationships += other.relationships;
        self.pending_relationships += other.pending_relationships;
        self.type_facts += other.type_facts;
        self.type_argument_usages += other.type_argument_usages;
        self.type_arguments += other.type_arguments;
        self.literals += other.literals;
        self.source_regions += other.source_regions;
        self.structural_facts += other.structural_facts;
        self.complexity_metrics += other.complexity_metrics;
        self.parse_diagnostics += other.parse_diagnostics;
        self.pending_resolutions += other.pending_resolutions;
        self.identifier_resolutions += other.identifier_resolutions;
    }

    pub fn from_extraction_rows(row_counts: &RowCounts) -> Self {
        Self {
            revision_file_changes: row_counts.revision_file_changes,
            files: row_counts.files,
            symbols: row_counts.symbols,
            symbol_annotations: row_counts.symbol_annotations,
            reference_sites: row_counts.reference_sites,
            identifiers: row_counts.identifiers,
            relationships: row_counts.relationships,
            pending_relationships: row_counts.pending_relationships,
            type_facts: row_counts.type_facts,
            type_argument_usages: row_counts.type_argument_usages,
            type_arguments: row_counts.type_arguments,
            literals: row_counts.literals,
            source_regions: row_counts.source_regions,
            structural_facts: row_counts.structural_facts,
            complexity_metrics: row_counts.complexity_metrics,
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
    /// A whole-workspace scan re-extracted every supported file to advance the
    /// durable reference-resolution evidence contract.
    ResolutionUpgraded,
    /// A resolver hook returned an error. Non-fatal: the scan still commits with
    /// the affected rows left unresolved, and the scan report records the message
    /// (design §"Failure semantics"). Not an `ERROR_CODES` member — it never fails
    /// the write.
    ResolutionFailed,
    /// `scan --parent-pid` observed that the named process is no longer this
    /// process's parent, so the scan aborted before writing the artifact.
    /// `details` carries `expected_parent_pid` and `observed_parent_pid`.
    ParentExited,
    /// `scan --spool-dir` resolved inside `--root`, so that directory and
    /// everything under it is excluded from the scan. Warning-only: the
    /// exclusion is correct, but it must not be silent.
    SpoolDirExcluded,
    /// `scan --spool-dir` could not take an ownership lock, so this scan's spool
    /// falls back to a name no later scan can ever remove. Warning-only: the
    /// scan is unaffected, but the leak protection the flag was adopted for is
    /// inert.
    SpoolLockUnavailable,
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
        Self::UnsupportedFormat,
        Self::UnsupportedFile,
        Self::ReadFailed,
        Self::ParseFailed,
        Self::DataLossGuard,
        Self::ExportFailed,
        Self::InternalError,
        Self::ParentExited,
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
