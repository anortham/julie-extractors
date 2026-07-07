#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOperation {
    Scan,
    Update,
    Delete,
}

impl WriteOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            WriteOperation::Scan => "scan",
            WriteOperation::Update => "update",
            WriteOperation::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Incremental,
    Force,
    SingleFile,
}

impl WriteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WriteMode::Incremental => "incremental",
            WriteMode::Force => "force",
            WriteMode::SingleFile => "single_file",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileStatus {
    Indexed,
    Unsupported,
    FailedPreserved,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Indexed => "indexed",
            FileStatus::Unsupported => "unsupported",
            FileStatus::FailedPreserved => "failed_preserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionChangeKind {
    Inserted,
    Updated,
    Deleted,
    Unsupported,
}

impl RevisionChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RevisionChangeKind::Inserted => "inserted",
            RevisionChangeKind::Updated => "updated",
            RevisionChangeKind::Deleted => "deleted",
            RevisionChangeKind::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionInput {
    pub operation: WriteOperation,
    pub mode: Option<WriteMode>,
    pub started_at: String,
    pub completed_at: String,
    pub binary_version: String,
    pub input_root: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowCounts {
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
    pub source_regions: i64,
    pub structural_facts: i64,
    pub complexity_metrics: i64,
    pub parse_diagnostics: i64,
    pub revision_file_changes: i64,
}

impl RowCounts {
    pub fn extraction_rows(&self) -> i64 {
        self.files
            + self.symbols
            + self.symbol_annotations
            + self.identifiers
            + self.relationships
            + self.pending_relationships
            + self.type_facts
            + self.type_argument_usages
            + self.type_arguments
            + self.literals
            + self.source_regions
            + self.structural_facts
            + self.complexity_metrics
            + self.parse_diagnostics
    }

    pub fn counts_json(&self) -> String {
        format!(
            concat!(
                "{{",
                r#""files":{},"symbols":{},"symbol_annotations":{},"identifiers":{},"#,
                r#""relationships":{},"pending_relationships":{},"type_facts":{},"#,
                r#""type_argument_usages":{},"type_arguments":{},"literals":{},"#,
                r#""source_regions":{},"structural_facts":{},"#,
                r#""complexity_metrics":{},"parse_diagnostics":{},"#,
                r#""revision_file_changes":{}"#,
                "}}"
            ),
            self.files,
            self.symbols,
            self.symbol_annotations,
            self.identifiers,
            self.relationships,
            self.pending_relationships,
            self.type_facts,
            self.type_argument_usages,
            self.type_arguments,
            self.literals,
            self.source_regions,
            self.structural_facts,
            self.complexity_metrics,
            self.parse_diagnostics,
            self.revision_file_changes
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteResult {
    pub revision_id: Option<i64>,
    pub rows_written: RowCounts,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub files_skipped: usize,
    pub transactions_committed: usize,
    /// Outcome of the in-transaction resolution hook for this write. `Default`
    /// (zero counts, no failure) when no hook ran or the hook was a no-op — every
    /// existing hookless caller therefore sees an empty resolution outcome.
    pub resolution: ResolutionWriteOutcome,
}

/// What the writer surfaces to callers about the resolution hook that ran inside
/// this write's transaction. The writer consumes only `ResolutionCounts` for
/// revision accounting; `failed` carries the hook's error message (design
/// §"Failure semantics") so the caller can record `ResolutionFailed` in its
/// report. The per-language `ResolutionReport` never travels through the writer —
/// Task 5's closure keeps it in its own captured state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionWriteOutcome {
    pub counts: crate::resolution_store::ResolutionCounts,
    pub failed: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtifactCapabilitySnapshot {
    pub parser_inventory: Vec<ArtifactParserInventoryRow>,
    pub languages: Vec<ArtifactLanguageCapabilityRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactParserInventoryRow {
    pub language: String,
    pub parser_package: String,
    pub parser_version: Option<String>,
    pub grammar_version: Option<String>,
    pub source: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactLanguageCapabilityRow {
    pub language: String,
    pub parser_package: String,
    pub extensions: Vec<String>,
    pub dependency_status: String,
    pub target_capabilities: ArtifactCapabilityFlags,
    pub actual_capabilities: ArtifactCapabilityFlags,
    pub kind_coverage: serde_json::Value,
    pub fixtures: Vec<ArtifactLanguageCapabilityFixtureRow>,
    pub gaps: Vec<ArtifactLanguageCapabilityGapRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtifactCapabilityFlags {
    pub symbols: bool,
    pub relationships: bool,
    pub pending_relationships: bool,
    pub identifiers: bool,
    pub types: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactLanguageCapabilityFixtureRow {
    pub fixture_name: String,
    pub source_path: String,
    pub expected_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactLanguageCapabilityGapRow {
    pub gap_id: String,
    pub capability: String,
    pub status: String,
    pub reason: String,
    pub required_closure: String,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactFile {
    pub file_id: String,
    pub path: String,
    pub language: String,
    pub content_hash: String,
    pub content_bytes: i64,
    pub line_count: Option<i64>,
    pub indexed_at: String,
    pub status: FileStatus,
    pub metadata_json: Option<String>,
    pub symbols: Vec<ArtifactSymbol>,
    pub symbol_annotations: Vec<ArtifactSymbolAnnotation>,
    pub identifiers: Vec<ArtifactIdentifier>,
    pub relationships: Vec<ArtifactRelationship>,
    pub pending_relationships: Vec<ArtifactPendingRelationship>,
    pub type_facts: Vec<ArtifactTypeFact>,
    pub type_argument_usages: Vec<ArtifactTypeArgumentUsage>,
    pub type_arguments: Vec<ArtifactTypeArgument>,
    pub literals: Vec<ArtifactLiteral>,
    pub source_regions: Vec<ArtifactSourceRegion>,
    pub structural_facts: Vec<ArtifactStructuralFact>,
    pub complexity_metrics: Vec<ArtifactComplexityMetric>,
    pub parse_diagnostics: Vec<ArtifactParseDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactSymbol {
    pub symbol_id: String,
    pub name: String,
    pub kind: String,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub visibility: Option<String>,
    pub parent_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub body_start_line: Option<i64>,
    pub body_start_column: Option<i64>,
    pub body_end_line: Option<i64>,
    pub body_end_column: Option<i64>,
    pub body_start_byte: Option<i64>,
    pub body_end_byte: Option<i64>,
    pub body_hash: Option<String>,
    pub semantic_group: Option<String>,
    pub confidence: Option<f64>,
    pub content_type: Option<String>,
    pub is_test: bool,
    pub test_container: bool,
    pub test_lifecycle: bool,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactSymbol {
    fn default() -> Self {
        Self {
            symbol_id: String::new(),
            name: String::new(),
            kind: "function".to_string(),
            signature: None,
            doc_comment: None,
            visibility: None,
            parent_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            body_start_line: None,
            body_start_column: None,
            body_end_line: None,
            body_end_column: None,
            body_start_byte: None,
            body_end_byte: None,
            body_hash: None,
            semantic_group: None,
            confidence: None,
            content_type: None,
            is_test: false,
            test_container: false,
            test_lifecycle: false,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSymbolAnnotation {
    pub annotation_id: String,
    pub symbol_id: String,
    pub annotation: String,
    pub annotation_key: String,
    pub raw_text: Option<String>,
    pub carrier: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactIdentifier {
    pub identifier_id: String,
    pub name: String,
    pub kind: String,
    pub containing_symbol_id: Option<String>,
    pub target_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub confidence: f64,
    pub code_context: Option<String>,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactIdentifier {
    fn default() -> Self {
        Self {
            identifier_id: String::new(),
            name: String::new(),
            kind: "call".to_string(),
            containing_symbol_id: None,
            target_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            confidence: 1.0,
            code_context: None,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRelationship {
    pub relationship_id: String,
    pub from_symbol_id: String,
    pub to_symbol_id: String,
    pub kind: String,
    pub start_line: Option<i64>,
    pub start_column: Option<i64>,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub confidence: f64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactRelationship {
    fn default() -> Self {
        Self {
            relationship_id: String::new(),
            from_symbol_id: String::new(),
            to_symbol_id: String::new(),
            kind: "calls".to_string(),
            start_line: None,
            start_column: None,
            end_line: None,
            end_column: None,
            start_byte: None,
            end_byte: None,
            confidence: 1.0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactPendingRelationship {
    pub pending_relationship_id: String,
    pub from_symbol_id: String,
    pub caller_scope_symbol_id: Option<String>,
    pub kind: String,
    pub target_display_name: String,
    pub target_terminal_name: String,
    pub target_receiver: Option<String>,
    pub target_namespace_json: String,
    pub target_import_context: Option<String>,
    pub start_line: i64,
    pub start_column: Option<i64>,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub confidence: f64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactPendingRelationship {
    fn default() -> Self {
        Self {
            pending_relationship_id: String::new(),
            from_symbol_id: String::new(),
            caller_scope_symbol_id: None,
            kind: "calls".to_string(),
            target_display_name: String::new(),
            target_terminal_name: String::new(),
            target_receiver: None,
            target_namespace_json: "[]".to_string(),
            target_import_context: None,
            start_line: 1,
            start_column: None,
            end_line: None,
            end_column: None,
            start_byte: None,
            end_byte: None,
            confidence: 1.0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTypeFact {
    pub type_fact_id: String,
    pub symbol_id: String,
    pub resolved_type: String,
    pub generic_params_json: Option<String>,
    pub constraints_json: Option<String>,
    pub is_inferred: bool,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTypeArgumentUsage {
    pub usage_id: String,
    pub identifier_id: String,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTypeArgument {
    pub type_argument_id: String,
    pub usage_id: String,
    pub parent_type_argument_id: Option<String>,
    pub ordinal: i64,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactLiteral {
    pub literal_id: String,
    pub literal_text: String,
    pub kind: String,
    pub carrier: Option<String>,
    pub arg_position: i64,
    pub containing_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub confidence: f64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactLiteral {
    fn default() -> Self {
        Self {
            literal_id: String::new(),
            literal_text: String::new(),
            kind: "other".to_string(),
            carrier: None,
            arg_position: 0,
            containing_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            confidence: 1.0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSourceRegion {
    pub source_region_id: String,
    pub kind: String,
    pub containing_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactSourceRegion {
    fn default() -> Self {
        Self {
            source_region_id: String::new(),
            kind: "comment".to_string(),
            containing_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactStructuralFact {
    pub structural_fact_id: String,
    pub pattern_id: String,
    pub capture_name: String,
    pub node_kind: String,
    pub containing_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub confidence: f64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactStructuralFact {
    fn default() -> Self {
        Self {
            structural_fact_id: String::new(),
            pattern_id: String::new(),
            capture_name: String::new(),
            node_kind: String::new(),
            containing_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            confidence: 1.0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactComplexityMetric {
    pub complexity_metric_id: String,
    pub scope: String,
    pub symbol_id: Option<String>,
    pub algorithm_id: String,
    pub covered_lines: i64,
    pub covered_bytes: i64,
    pub decision_count: i64,
    pub loop_count: i64,
    pub max_nesting_depth: i64,
    pub parameter_count: Option<i64>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub metadata_json: Option<String>,
}

impl Default for ArtifactComplexityMetric {
    fn default() -> Self {
        Self {
            complexity_metric_id: String::new(),
            scope: "file".to_string(),
            symbol_id: None,
            algorithm_id: "julie-ast-complexity-v1".to_string(),
            covered_lines: 0,
            covered_bytes: 0,
            decision_count: 0,
            loop_count: 0,
            max_nesting_depth: 0,
            parameter_count: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            start_byte: 0,
            end_byte: 0,
            metadata_json: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactParseDiagnostic {
    pub diagnostic_id: String,
    pub kind: String,
    pub message: Option<String>,
    pub start_line: i64,
    pub start_column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub metadata_json: Option<String>,
}
use serde::{Deserialize, Serialize};
