// Base Extractor Types and Traits for Julie
//
// This module is a precise Implementation of base-extractor.ts (404 lines).
// Every method, utility, and algorithm has been carefully ported to maintain exact functionality.
//
// CRITICAL: This represents months of development work. Any changes must maintain
// 100% functional parity with extractors and pass all tests.
//
// Refactored from monolithic 1090-line file into modular structure:
// - types.rs: All data structures (Symbol, Identifier, Relationship, TypeInfo, etc.)
// - extractor.rs: BaseExtractor implementation (core methods)
// - tree_methods.rs: Tree navigation and traversal methods

pub mod annotations;
pub mod body;
pub mod code_structural_facts;
pub mod complexity_metrics;
pub mod config_literals;
pub mod containing_symbol;
pub mod creation_methods;
pub mod data_structural_facts;
pub mod embedded_span;
pub mod extractor;
pub mod framework_structural_facts;
pub mod http_boundary;
pub mod kinds;
pub mod marker_structural_facts;
mod markup_scan;
pub mod relationship_resolution;
mod results_normalization;
mod rust_doc_test_facts;
pub mod source_regions;
pub mod span;
mod sql_structural_facts;
mod string_literals;
pub mod structural_fact_registry;
pub mod structural_facts;
pub mod tree_methods;
pub mod type_arguments;
pub mod type_models;
pub mod types;
pub mod web_structural_facts;

// Re-export key types for external use
pub use annotations::normalize_annotations;
pub use body::BodySpan;
pub use code_structural_facts::collect_code_structural_facts;
pub use complexity_metrics::collect_complexity_metrics;
pub(crate) use containing_symbol::attach_containing_symbols;
pub use data_structural_facts::collect_data_structural_facts;
pub use embedded_span::EmbeddedSpanOffset;
pub use extractor::BaseExtractor;
pub use framework_structural_facts::collect_framework_structural_facts;
pub use kinds::{IdentifierKind, RelationshipKind, SymbolKind, TestRole, Visibility};
pub use marker_structural_facts::collect_marker_structural_facts;
pub use relationship_resolution::{
    LocalTargetResolution, ScopedSymbolIndex, StructuredPendingRelationship, UnresolvedTarget,
};
pub(crate) use rust_doc_test_facts::collect_rust_doc_test_facts;
pub use source_regions::collect_source_regions;
pub use span::{NormalizedSpan, RecordOffset, normalize_file_path};
pub use sql_structural_facts::collect_sql_structural_facts;
pub use structural_fact_registry::{
    KeyPresence, MetadataKeySpec, MetadataValueType, StructuralFactPatternSpec,
    structural_fact_pattern_specs, structural_fact_patterns_contract_json,
    structural_fact_patterns_json,
};
pub use structural_facts::collect_structural_facts;
pub use tree_methods::{find_child_by_type, find_child_by_types};
pub use type_arguments::{TypeArgDecomposer, extract_type_arguments};
pub use type_models::{Literal, LiteralKind, TypeArgument, TypeArgumentUsage};
pub use types::{
    AnnotationMarker, ComplexityMetric, ContextConfig, ExtractionLevel, ExtractionResults,
    Identifier, ParseDiagnostic, ParseDiagnosticKind, PendingRelationship, Relationship,
    SourceRegion, SourceRegionKind, StructuralFact, Symbol, SymbolOptions, TypeInfo,
};
pub use web_structural_facts::collect_web_structural_facts;

pub(crate) fn containing_symbol_at_line(symbols: &[Symbol], line_number: u32) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.start_line <= line_number && symbol.end_line >= line_number)
        .min_by_key(|symbol| symbol.end_line.saturating_sub(symbol.start_line))
}
