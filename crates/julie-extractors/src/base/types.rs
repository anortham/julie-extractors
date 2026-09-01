// Base Extractor Types for Julie
//
// All data structures for symbol extraction, identifiers, relationships, and types.
// Lines 15-394 from original base.rs

use md5;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::body::BodySpan;
use super::relationship_resolution::StructuredPendingRelationship;
use super::span::NormalizedSpan;
use super::type_models::{Literal, TypeArgumentUsage};

pub use super::kinds::{IdentifierKind, RelationshipKind, SymbolKind, TestRole, Visibility};

/// Why a file's extraction is incomplete: tree-sitter parse recovery, or the
/// crate-wide tree traversal budget cutting a pathologically deep tree short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseDiagnosticKind {
    Error,
    Missing,
    DepthTruncated,
}

/// Span for syntax recovery produced by tree-sitter.
///
/// `message` is `None` for the spans tree-sitter reports directly — the node
/// kind and span are the whole fact. An extractor sets it when it knows
/// something about the failure the tree cannot express, such as an
/// error-recovery pass that stopped before it ran out of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    pub kind: ParseDiagnosticKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: u32,
    pub end_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceRegionKind {
    Comment,
    DocComment,
    StringLiteral,
    Embedded,
}

impl SourceRegionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::DocComment => "doc_comment",
            Self::StringLiteral => "string_literal",
            Self::Embedded => "embedded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRegion {
    pub id: String,
    pub file_path: String,
    pub language: String,
    pub kind: SourceRegionKind,
    pub containing_symbol_id: Option<String>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl SourceRegion {
    pub fn apply_normalized_span(&mut self, span: NormalizedSpan) {
        self.start_line = span.start_line;
        self.start_column = span.start_column;
        self.end_line = span.end_line;
        self.end_column = span.end_column;
        self.start_byte = span.start_byte;
        self.end_byte = span.end_byte;
    }

    pub fn refresh_id(&mut self) {
        self.id = stable_location_id(self.file_path.as_str(), self.kind.as_str(), self.span());
    }

    fn span(&self) -> NormalizedSpan {
        NormalizedSpan {
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralFact {
    pub id: String,
    pub file_path: String,
    pub language: String,
    pub pattern_id: String,
    pub capture_name: String,
    pub node_kind: String,
    pub containing_symbol_id: Option<String>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl StructuralFact {
    pub fn apply_normalized_span(&mut self, span: NormalizedSpan) {
        self.start_line = span.start_line;
        self.start_column = span.start_column;
        self.end_line = span.end_line;
        self.end_column = span.end_column;
        self.start_byte = span.start_byte;
        self.end_byte = span.end_byte;
    }

    pub fn refresh_id(&mut self) {
        self.id = stable_location_id(
            self.file_path.as_str(),
            &format!("{}:{}", self.pattern_id, self.capture_name),
            self.span(),
        );
    }

    fn span(&self) -> NormalizedSpan {
        NormalizedSpan {
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComplexityMetric {
    pub id: String,
    pub file_path: String,
    pub language: String,
    pub scope: String,
    pub symbol_id: Option<String>,
    pub algorithm_id: String,
    pub covered_lines: u32,
    pub covered_bytes: u32,
    pub decision_count: u32,
    pub loop_count: u32,
    pub max_nesting_depth: u32,
    pub parameter_count: Option<u32>,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl ComplexityMetric {
    pub fn apply_normalized_span(&mut self, span: NormalizedSpan) {
        self.start_line = span.start_line;
        self.start_column = span.start_column;
        self.end_line = span.end_line;
        self.end_column = span.end_column;
        self.start_byte = span.start_byte;
        self.end_byte = span.end_byte;
    }

    pub fn refresh_id(&mut self) {
        let identity = self.symbol_id.as_deref().unwrap_or("file");
        self.id = stable_location_id(
            self.file_path.as_str(),
            &format!("complexity:{}:{}", self.scope, identity),
            self.span(),
        );
    }

    fn span(&self) -> NormalizedSpan {
        NormalizedSpan {
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

/// Canonical annotation marker with display, match, and source text forms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnotationMarker {
    pub annotation: String,
    pub annotation_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier: Option<String>,
}

/// Configuration for code context extraction
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Number of lines to show before the symbol
    pub lines_before: usize,
    /// Number of lines to show after the symbol
    pub lines_after: usize,
    /// Maximum line length to display (longer lines get truncated)
    pub max_line_length: usize,
    /// Whether to show line numbers in context
    pub show_line_numbers: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            lines_before: 3,
            lines_after: 3,
            max_line_length: 120,
            show_line_numbers: true,
        }
    }
}

/// A code symbol (function, class, variable, etc.) extracted from source code
///
/// Direct Implementation of Symbol interface - exact field mapping maintained
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Symbol {
    /// Unique identifier for this symbol (MD5 hash standard format)
    pub id: String,
    /// Symbol name as it appears in code
    pub name: String,
    /// Kind of symbol (function, class, etc.)
    pub kind: SymbolKind,
    /// Programming language this symbol is from
    pub language: String,
    /// File path where this symbol is defined
    pub file_path: String,
    /// Start line number (1-based, exactly standard format)
    pub start_line: u32,
    /// Start column number (0-based, exactly standard format)
    pub start_column: u32,
    /// End line number (1-based, exactly standard format)
    pub end_line: u32,
    /// End column number (0-based, exactly standard format)
    pub end_column: u32,
    /// Start byte offset in file
    pub start_byte: u32,
    /// End byte offset in file
    pub end_byte: u32,
    /// Body span for body-bearing symbols.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_span: Option<BodySpan>,
    /// Formatting-insensitive hash of the body span token stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_hash: Option<String>,
    /// Function/method signature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Documentation comment (using extraction algorithm)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    /// Visibility (public, private, protected)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    /// Parent symbol ID (for methods in classes, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Additional language-specific metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Canonical annotation markers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<AnnotationMarker>,
    /// Semantic group for cross-language linking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_group: Option<String>,
    /// Confidence score for symbol extraction (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Code context lines around the symbol (3 lines before + match + 3 lines after)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_context: Option<String>,
    /// Content type to distinguish documentation from code
    /// None = code (default), Some("documentation") = markdown docs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

impl Symbol {
    pub fn apply_normalized_span(&mut self, span: NormalizedSpan) {
        self.start_line = span.start_line;
        self.start_column = span.start_column;
        self.end_line = span.end_line;
        self.end_column = span.end_column;
        self.start_byte = span.start_byte;
        self.end_byte = span.end_byte;
    }

    pub fn refresh_id(&mut self) -> String {
        let previous_id = self.id.clone();
        self.id = stable_location_id(self.file_path.as_str(), self.name.as_str(), self.span());
        previous_id
    }

    fn span(&self) -> NormalizedSpan {
        NormalizedSpan {
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

/// An identifier (reference/usage) extracted from source code
///
/// Unlike Symbols (definitions), Identifiers represent usage sites like function calls,
/// variable references, type usages, etc. They are extracted unresolved (target_symbol_id is None)
/// and resolved on-demand during queries for optimal incremental update performance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Identifier {
    /// Unique identifier for this reference (MD5 hash)
    pub id: String,
    /// Identifier name as it appears in code
    pub name: String,
    /// Kind of identifier (call, variable_ref, type_usage, member_access)
    pub kind: IdentifierKind,
    /// Programming language this identifier is from
    pub language: String,
    /// File path where this identifier appears
    pub file_path: String,
    /// Start line number (1-based)
    pub start_line: u32,
    /// Start column number (0-based)
    pub start_column: u32,
    /// End line number (1-based)
    pub end_line: u32,
    /// End column number (0-based)
    pub end_column: u32,
    /// Start byte offset in file
    pub start_byte: u32,
    /// End byte offset in file
    pub end_byte: u32,
    /// ID of the symbol that contains this identifier (e.g., which function uses this variable)
    pub containing_symbol_id: Option<String>,
    /// ID of the symbol this identifier refers to (None until resolved on-demand)
    pub target_symbol_id: Option<String>,
    /// Confidence score for identifier extraction (0.0 to 1.0)
    pub confidence: f32,
    /// Enclosing type name, recorded only when the call's receiver is the
    /// language's self reference (`this`/`base`). Rides into the artifact
    /// identifier `metadata_json` under key `"receiver_type"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_type: Option<String>,
    /// Always `None`. Per-identifier code context was write-only dead weight —
    /// roughly half of all identifier bytes in the scan spool and the artifact —
    /// and no consumer ever read it, so extractors stopped populating it.
    pub code_context: Option<String>,
}

impl Identifier {
    pub fn apply_normalized_span(&mut self, span: NormalizedSpan) {
        self.start_line = span.start_line;
        self.start_column = span.start_column;
        self.end_line = span.end_line;
        self.end_column = span.end_column;
        self.start_byte = span.start_byte;
        self.end_byte = span.end_byte;
    }

    pub fn refresh_id(&mut self) {
        self.id = stable_location_id(self.file_path.as_str(), self.name.as_str(), self.span());
    }

    fn span(&self) -> NormalizedSpan {
        NormalizedSpan {
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

pub(crate) fn stable_location_id(file_path: &str, name: &str, span: NormalizedSpan) -> String {
    let input = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        file_path,
        name,
        span.start_line,
        span.start_column,
        span.end_line,
        span.end_column,
        span.start_byte,
        span.end_byte
    );
    format!("{:x}", md5::compute(input.as_bytes()))
}

/// Relationship between two symbols - reference implementation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    /// Unique identifier for this relationship
    pub id: String,
    /// Source symbol ID
    #[serde(rename = "fromSymbolId")]
    pub from_symbol_id: String,
    /// Target symbol ID
    #[serde(rename = "toSymbolId")]
    pub to_symbol_id: String,
    /// Type of relationship
    pub kind: RelationshipKind,
    /// File where this relationship occurs
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// Line number where relationship occurs (1-based standard format)
    #[serde(rename = "lineNumber")]
    pub line_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<NormalizedSpan>,
    #[serde(default)]
    pub reference_site_is_exact: bool,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f32,
    /// Additional metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// A pending relationship that needs cross-file resolution after indexing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingRelationship {
    #[serde(rename = "fromSymbolId")]
    pub from_symbol_id: String,
    #[serde(rename = "calleeName")]
    pub callee_name: String,
    pub kind: RelationshipKind,
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "lineNumber")]
    pub line_number: u32,
    pub confidence: f32,
}

/// Type information for a symbol - reference implementation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypeInfo {
    /// Symbol this type info belongs to
    #[serde(rename = "symbolId")]
    pub symbol_id: String,
    /// Resolved type name
    #[serde(rename = "resolvedType")]
    pub resolved_type: String,
    /// Generic type parameters
    #[serde(rename = "genericParams")]
    pub generic_params: Option<Vec<String>>,
    /// Type constraints
    pub constraints: Option<Vec<String>>,
    /// Whether type was inferred or explicit
    #[serde(rename = "isInferred")]
    pub is_inferred: bool,
    /// Programming language
    pub language: String,
    /// Additional type metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Per-language declared-type decorations that [`strip_type_decorations`]
/// removes to produce the base type name Miller matches verbatim against
/// type-like symbol names.
#[derive(Debug, Clone, Copy)]
pub struct TypeNameRules {
    /// Suffixes marking nullability, stripped from the end (for example `?`).
    pub nullable_suffixes: &'static [&'static str],
    /// By-ref, pointer, and borrow markers stripped from the front (for
    /// example `ref`, `out`, `in`, `&`, `*`, `mut`). A prefix that ends in a
    /// letter or digit only matches when whitespace follows it.
    pub reference_prefixes: &'static [&'static str],
    /// Characters that open a generic argument list; the name is cut at the
    /// first one (for example `<`).
    pub generic_open: &'static [char],
}

/// Reduce declared type text to the base type name: drop reference prefixes,
/// generic argument lists, and nullable suffixes. Array suffixes (`[]`) and
/// namespace qualifiers stay untouched.
pub fn strip_type_decorations(declared: &str, rules: &TypeNameRules) -> String {
    let mut name = declared.trim();

    'prefixes: loop {
        for prefix in rules.reference_prefixes {
            let Some(rest) = name.strip_prefix(prefix) else {
                continue;
            };
            let word_prefix = prefix.ends_with(|c: char| c.is_alphanumeric());
            if word_prefix && !rest.starts_with(char::is_whitespace) {
                continue;
            }
            name = rest.trim_start();
            continue 'prefixes;
        }
        break;
    }

    if let Some(generic_start) = name.find(rules.generic_open) {
        name = name[..generic_start].trim_end();
    }

    'suffixes: loop {
        for suffix in rules.nullable_suffixes {
            if let Some(rest) = name.strip_suffix(suffix) {
                name = rest.trim_end();
                continue 'suffixes;
            }
        }
        break;
    }

    name.to_string()
}

/// Options for creating symbols - matches createSymbol options
#[derive(Debug, Clone, Default)]
pub struct SymbolOptions {
    pub signature: Option<String>,
    pub visibility: Option<Visibility>,
    pub parent_id: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub doc_comment: Option<String>,
    pub annotations: Vec<AnnotationMarker>,
}

/// How much of the extraction surface a scan materializes.
///
/// `Symbols` is the progressive-indexing first-open level: the identifier
/// walk, its byproducts (literals, type-argument usages), and the text/facts
/// collectors (source regions, structural facts) never run. `Full` is the
/// complete extraction — the default everywhere a level is not requested, so
/// pre-levels callers and artifacts are unaffected.
///
/// The level is uniform across all supported languages by construction: the
/// gate lives in the shared registry dispatch, and
/// [`ExtractionResults::strip_to_symbols_level`] is the single authority on
/// which result families a `Symbols` extraction may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionLevel {
    Symbols,
    Full,
}

impl ExtractionLevel {
    pub fn includes_references(self) -> bool {
        matches!(self, ExtractionLevel::Full)
    }

    /// Canonical `artifact_metadata.index_level` value for this level.
    pub fn metadata_value(self) -> &'static str {
        match self {
            ExtractionLevel::Symbols => "symbols",
            ExtractionLevel::Full => "full",
        }
    }

    pub fn from_metadata_value(value: &str) -> Option<Self> {
        match value {
            "symbols" => Some(ExtractionLevel::Symbols),
            "full" => Some(ExtractionLevel::Full),
            _ => None,
        }
    }
}

/// Extraction results - matches getResults return type
#[derive(Debug, Clone)]
pub struct ExtractionResults {
    pub symbols: Vec<Symbol>,
    pub relationships: Vec<Relationship>,
    /// Pending relationships that need cross-file resolution after workspace indexing
    pub pending_relationships: Vec<PendingRelationship>,
    /// Structured pending relationships preserve unresolved call context.
    pub structured_pending_relationships: Vec<StructuredPendingRelationship>,
    pub types: HashMap<String, TypeInfo>,
    pub identifiers: Vec<Identifier>, // Include identifiers for LSP-quality tools
    /// Ordered/nested generic type arguments captured at use sites (Miller
    /// bridge Phase 2). Carried out of the extractor's `BaseExtractor` so the
    /// indexing layer can flatten and persist them. Keyed to a use-site
    /// identifier by `identifier_id`.
    pub type_argument_usages: Vec<TypeArgumentUsage>,
    /// String literals captured at call-argument sites (Miller bridge Phase 3),
    /// config-free (carrier set, kind = Other). The indexing layer classifies +
    /// gates these by carrier before persistence.
    pub literals: Vec<Literal>,
    pub source_regions: Vec<SourceRegion>,
    pub structural_facts: Vec<StructuralFact>,
    pub complexity_metrics: Vec<ComplexityMetric>,
    pub parse_diagnostics: Vec<ParseDiagnostic>,
}

#[cfg(test)]
mod strip_type_decorations_tests {
    use super::{TypeNameRules, strip_type_decorations};

    const RULES: TypeNameRules = TypeNameRules {
        nullable_suffixes: &["?"],
        reference_prefixes: &["ref", "out", "in", "&", "*", "mut"],
        generic_open: &['<'],
    };

    #[test]
    fn strip_type_decorations_reduces_declared_text_to_the_base_type_name() {
        let cases = [
            ("List<int>", "List"),
            (
                "IReadOnlyDictionary<string, IReadOnlyList<GraphNeighbour>>",
                "IReadOnlyDictionary",
            ),
            ("GraphTraversal?", "GraphTraversal"),
            ("ref Foo", "Foo"),
            ("&mut Foo", "Foo"),
            ("*Store", "Store"),
            ("string[]", "string[]"),
            ("Foo.Bar", "Foo.Bar"),
        ];

        for (declared, expected) in cases {
            assert_eq!(
                strip_type_decorations(declared, &RULES),
                expected,
                "declared `{declared}` should normalize to `{expected}`"
            );
        }
    }

    #[test]
    fn strip_type_decorations_keeps_names_that_merely_start_with_a_word_prefix() {
        assert_eq!(strip_type_decorations("int", &RULES), "int");
        assert_eq!(strip_type_decorations("reference", &RULES), "reference");
    }
}
