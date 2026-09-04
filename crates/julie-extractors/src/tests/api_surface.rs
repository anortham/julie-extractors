use crate::base::BaseExtractor;
use crate::language::{
    detect_language_from_extension, language_spec, language_specs,
    supported_extensions as language_supported_extensions,
    supported_languages as language_supported_languages,
};
use crate::registry::supported_languages as registry_supported_languages;
use crate::{
    ExtractionLevel, PendingRelationship, RelationshipKind, extract_canonical, extract_canonical_at,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn test_public_contract_version_marks_current_fact_families() {
    let version = crate::EXTRACTION_CONTRACT_VERSION;
    let extraction_identity_epoch: u32 = crate::EXTRACTION_IDENTITY_EPOCH;
    let _structural_fact: Option<crate::StructuralFact> = None;
    let _complexity_metric: Option<crate::ComplexityMetric> = None;

    assert_eq!(extraction_identity_epoch, 9);

    for marker in [
        "source-regions-v1",
        "structural-facts-v1",
        "complexity-metrics-v1",
        "file-derived-component-symbols-v1",
        "framework-route-facts-v1",
        "react-nextjs-route-facts-v1",
        "nuxt-route-facts-v1",
        "web-route-facts-v3",
        "http-boundary-facts-v1",
        "containing-symbol-binding-v2",
        "backend-http-boundary-v1",
        "backend-http-boundary-v2",
        "sql-tsql-facts-v1",
        "test-role-strings-v2",
        "csharp-visibility-v2",
        "go-subtests-v1",
        "rust-doc-test-facts-v1",
        "fsharp-v1",
        "marker-razorback-v1",
        "receiver-type-facts-v1",
        "receiver-type-facts-v2",
    ] {
        assert!(
            version.contains(marker),
            "EXTRACTION_CONTRACT_VERSION must include `{marker}` after the public extraction shape changes; got `{version}`"
        );
    }

    let crate_docs = include_str!("../lib.rs");
    for fact_family in ["source regions", "structural facts", "complexity metrics"] {
        assert!(
            crate_docs.contains(fact_family),
            "crate docs must advertise `{fact_family}` as part of ExtractionResults"
        );
    }
}

#[test]
fn test_public_api_surface_projects_canonical_results() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let file_path = "src/app.ts";
    let content = r#"
import { externalHelper } from "./external";

export function localHelper(input: number): number {
    return input + 1;
}

export function processData(): number {
    return localHelper(externalHelper(41));
}
"#;

    let canonical = extract_canonical(file_path, content, &workspace_root)
        .expect("canonical extraction should succeed");
    let canonical_at =
        extract_canonical_at(file_path, content, &workspace_root, ExtractionLevel::Full)
            .expect("canonical extraction at full level should succeed");

    assert!(
        !canonical.structured_pending_relationships.is_empty(),
        "parity coverage should exercise a canonical result with structured unresolved relationships"
    );
    assert_eq!(
        canonical.pending_relationships,
        canonical
            .structured_pending_relationships
            .clone()
            .into_iter()
            .map(|pending| pending.into_pending_relationship())
            .collect::<Vec<_>>(),
        "canonical extraction should keep the degraded compatibility payload aligned with structured unresolved entries"
    );

    assert_eq!(canonical_at.symbols, canonical.symbols);
    assert_eq!(canonical_at.identifiers, canonical.identifiers);
    assert_eq!(canonical_at.relationships, canonical.relationships);
    assert_eq!(
        canonical_at.pending_relationships,
        canonical.pending_relationships
    );
    assert_eq!(
        canonical_at.structured_pending_relationships,
        canonical.structured_pending_relationships
    );
    assert_eq!(canonical_at.types, canonical.types);
}

#[test]
fn test_public_api_surface_preserves_structured_pending_for_remaining_registry_wave() {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let cases = [
        (
            "src/main.c",
            r#"
int main_function() {
    int result = helper_function(21);
    return result;
}
"#,
            "helper_function",
            None,
        ),
        (
            "src/main.cpp",
            r#"
int main_function() {
    int result = helper_function(21);
    return result;
}
"#,
            "helper_function",
            None,
        ),
        (
            "src/processor.rs",
            r#"
pub fn process() -> i32 {
    let calc = Calculator::new(21);
    calc.double()
}
"#,
            "calc.double",
            Some("calc"),
        ),
        (
            "src/main.zig",
            r#"
const util = @import("util.zig");

fn main_function() i32 {
    const result = util.helper_function(21);
    return result;
}
"#,
            "util.helper_function",
            Some("util"),
        ),
        (
            "main.go",
            r#"
package main

import "myapp/utils"

func MainFunction() int {
    result := utils.HelperFunction(21)
    return result
}
"#,
            "utils.HelperFunction",
            Some("utils"),
        ),
        (
            "src/Widget.vue",
            r#"
<template>
  <section>
    <HeaderBar />
  </section>
</template>

<script setup lang="ts">
const title = format("Worker");

function format(value: string): string {
  return value.trim();
}
</script>
"#,
            "HeaderBar",
            None,
        ),
        (
            "lib/processor.py",
            r#"
def process():
    calc = Calculator(21)
    return calc.double()
"#,
            "calc.double",
            Some("calc"),
        ),
        (
            "lib/processor.rb",
            r#"
def process
  calc = Calculator.new(21)
  result = calc.double()
  result
end
"#,
            "Calculator.new",
            Some("Calculator"),
        ),
        (
            "lib/processor.gd",
            r#"
func process():
    var result = external_helper(21)
    return result
"#,
            "external_helper",
            None,
        ),
        (
            "lib/processor.dart",
            r#"
import 'utils.dart';

int mainFunction() {
    final result = helperFunction(21);
    return result;
}
"#,
            "helperFunction",
            None,
        ),
    ];

    for (file_path, content, expected_display_name, expected_receiver) in cases {
        let canonical =
            extract_canonical(file_path, content, &workspace_root).unwrap_or_else(|err| {
                panic!("canonical extraction should succeed for {file_path}: {err}")
            });

        let structured_pending = canonical
            .structured_pending_relationships
            .iter()
            .find(|pending| pending.target.display_name == expected_display_name)
            .unwrap_or_else(|| {
                panic!(
                    "canonical extraction should preserve structured pending target {expected_display_name} for {file_path}; found {:?}",
                    canonical
                        .structured_pending_relationships
                        .iter()
                        .map(|pending| pending.target.display_name.as_str())
                        .collect::<Vec<_>>()
                )
            });

        assert_eq!(
            structured_pending.target.receiver.as_deref(),
            expected_receiver,
            "canonical extraction should preserve receiver context for {file_path}"
        );
        assert_eq!(
            canonical.pending_relationships,
            canonical
                .structured_pending_relationships
                .clone()
                .into_iter()
                .map(|pending| pending.into_pending_relationship())
                .collect::<Vec<_>>(),
            "canonical extraction should keep degraded compatibility payload aligned for {file_path}"
        );
    }
}

#[test]
fn test_language_specs_drive_public_metadata_surfaces() {
    let spec_languages: Vec<_> = language_specs().iter().map(|spec| spec.name).collect();
    let registry_languages = registry_supported_languages();

    assert_eq!(
        registry_languages, spec_languages,
        "registry language order should come from the language spec table"
    );
    assert_eq!(
        language_supported_languages(),
        spec_languages.as_slice(),
        "public supported languages should come from the language spec table"
    );

    let spec_extensions: BTreeSet<_> = language_specs()
        .iter()
        .flat_map(|spec| spec.extensions.iter().copied())
        .collect();
    let public_extensions: BTreeSet<_> = language_supported_extensions().iter().copied().collect();
    assert_eq!(
        public_extensions, spec_extensions,
        "public supported extensions should come from the language spec table"
    );

    for spec in language_specs() {
        assert_eq!(
            language_spec(spec.name).map(|found| found.name),
            Some(spec.name),
            "canonical language should resolve its own spec"
        );
        for alias in spec.aliases {
            assert_eq!(
                language_spec(alias).map(|found| found.name),
                Some(spec.name),
                "alias {alias} should resolve to {}",
                spec.name
            );
        }
        for extension in spec.extensions {
            assert_eq!(
                detect_language_from_extension(extension),
                Some(spec.name),
                "extension {extension} should resolve to {}",
                spec.name
            );
        }
    }

    assert_eq!(language_spec("jsx").map(|spec| spec.name), Some("jsx"));
    assert_eq!(language_spec("tsx").map(|spec| spec.name), Some("tsx"));
    assert_eq!(language_spec("vue").map(|spec| spec.name), Some("vue"));
    assert_eq!(language_spec("vbnet").map(|spec| spec.name), Some("vbnet"));
    assert_eq!(detect_language_from_extension("vb"), Some("vbnet"));
    assert_eq!(detect_language_from_extension("qmltypes"), Some("qml"));
}

#[test]
fn test_msbuild_and_dotnet_xml_extensions_resolve_to_xml() {
    for extension in [
        "csproj", "props", "targets", "vbproj", "fsproj", "slnx", "nuspec", "resx",
    ] {
        assert_eq!(
            detect_language_from_extension(extension),
            Some("xml"),
            "extension {extension} should resolve to xml"
        );
    }

    assert_eq!(
        detect_language_from_extension("sln"),
        None,
        "sln is not XML and must stay unclaimed"
    );
}

#[test]
fn test_rust_language_spec_recognizes_inner_doc_comments() {
    let spec = language_spec("rust").expect("rust spec should exist");

    assert!(spec.is_doc_comment("/// outer line docs"));
    assert!(spec.is_doc_comment("//! inner line docs"));
    assert!(spec.is_doc_comment("/** outer block docs */"));
    assert!(spec.is_doc_comment("/*! inner block docs */"));
}

#[test]
fn test_base_extractor_owns_pending_relationship_storage() {
    let workspace_root = PathBuf::from("/test/workspace");
    let mut base = BaseExtractor::new(
        "rust".to_string(),
        "/test/workspace/src/lib.rs".to_string(),
        "fn main() {}".to_string(),
        &workspace_root,
    );
    let pending = PendingRelationship {
        from_symbol_id: "caller".to_string(),
        callee_name: "external".to_string(),
        kind: RelationshipKind::Calls,
        file_path: "src/lib.rs".to_string(),
        line_number: 1,
        confidence: 0.75,
    };

    base.add_pending_relationship(pending.clone());

    assert_eq!(base.get_pending_relationships(), vec![pending]);
    assert_eq!(base.get_structured_pending_relationships(), Vec::new());

    base.clear_pending_relationships();

    assert!(base.get_pending_relationships().is_empty());
    assert!(base.get_structured_pending_relationships().is_empty());
}

#[test]
fn test_public_api_surface_exports_exact_symbols() {
    let lib_rs = include_str!("../lib.rs");

    // Modules that must be pub(crate)
    for module in [
        "base",
        "registry",
        "pipeline",
        "test_detection",
        "test_calls",
        "utils",
        "language",
    ] {
        assert!(
            lib_rs.contains(&format!("pub(crate) mod {module};")),
            "module `{module}` must be declared pub(crate)"
        );
    }

    // All 38 language modules must be pub(crate)
    let languages = [
        "bash",
        "c",
        "cpp",
        "csharp",
        "css",
        "dart",
        "elixir",
        "erlang",
        "fsharp",
        "gdscript",
        "go",
        "html",
        "java",
        "javascript",
        "json",
        "kotlin",
        "lua",
        "markdown",
        "php",
        "powershell",
        "python",
        "qml",
        "qmldir",
        "r",
        "razor",
        "regex",
        "ruby",
        "rust",
        "scala",
        "sql",
        "swift",
        "toml",
        "typescript",
        "vbnet",
        "vue",
        "xml",
        "yaml",
        "zig",
    ];
    assert_eq!(languages.len(), 38);
    for lang in languages {
        assert!(
            lib_rs.contains(&format!("pub(crate) mod {lang};")),
            "language module `{lang}` must be declared pub(crate)"
        );
    }

    // Removed from root exports
    for removed in [
        "BaseExtractor",
        "is_test_symbol",
        "detect_language_from_extension",
        "get_tree_sitter_language",
        "LanguageRegistryEntry",
    ] {
        for line in lib_rs.lines() {
            if line.trim_start().starts_with("pub use") {
                assert!(
                    !line.contains(removed),
                    "removed symbol `{removed}` must not be re-exported with pub use; found in `{line}`"
                );
            }
        }
    }

    // Canonical extraction functions at root
    let _ = crate::extract_canonical
        as fn(&str, &str, &std::path::Path) -> Result<crate::ExtractionResults, anyhow::Error>;
    let _ = crate::extract_canonical_at
        as fn(
            &str,
            &str,
            &std::path::Path,
            crate::ExtractionLevel,
        ) -> Result<crate::ExtractionResults, anyhow::Error>;
    let _ = crate::extract_canonical_for_language_at
        as fn(
            &str,
            &str,
            &str,
            &std::path::Path,
            crate::ExtractionLevel,
        ) -> Result<crate::ExtractionResults, anyhow::Error>;

    // Language detection at root
    let _ = crate::detect_language_for_path as fn(&std::path::Path, &str) -> Option<&'static str>;
    let _ = crate::detect_language_for_source as fn(&str, &str) -> Option<&'static str>;

    // Registry and capabilities at root
    let _ = crate::supported_languages as fn() -> Vec<&'static str>;
    let _ = crate::capability_snapshot as fn() -> &'static crate::CapabilitySnapshot;

    // Policy and serializer helpers at root
    let _ = crate::classify_literals_by_carrier;
    let _ = crate::structural_fact_patterns_json as fn() -> serde_json::Value;
    let _ = crate::extract_type_arguments;
    let _ = crate::normalize_annotations::<&str>;

    // All row types, enums, and markers reachable at root
    fn assert_types<
        Symbol: 'static,
        Relationship: 'static,
        PendingRelationship: 'static,
        Identifier: 'static,
        Literal: 'static,
        TypeInfo: 'static,
        TypeArgument: 'static,
        TypeArgumentUsage: 'static,
        SourceRegion: 'static,
        ParseDiagnostic: 'static,
        ComplexityMetric: 'static,
        NormalizedSpan: 'static,
        StructuralFact: 'static,
        StructuredPendingRelationship: 'static,
        ExtractionResults: 'static,
        ExtractionLevel: 'static,
        SymbolKind: 'static,
        SymbolOptions: 'static,
        RelationshipKind: 'static,
        IdentifierKind: 'static,
        LiteralKind: 'static,
        SourceRegionKind: 'static,
        ParseDiagnosticKind: 'static,
        AnnotationMarker: 'static,
        TestRole: 'static,
        Visibility: 'static,
        LanguageCapabilities: 'static,
        CapabilitySnapshot: 'static,
        CapabilityFlags: 'static,
        CapabilityGap: 'static,
        CapabilityKindCoverage: 'static,
        CapabilityRow: 'static,
        FixtureRef: 'static,
        KindCoverage: 'static,
        KindCoverageGap: 'static,
    >() {
    }

    assert_types::<
        crate::Symbol,
        crate::Relationship,
        crate::PendingRelationship,
        crate::Identifier,
        crate::Literal,
        crate::TypeInfo,
        crate::TypeArgument,
        crate::TypeArgumentUsage,
        crate::SourceRegion,
        crate::ParseDiagnostic,
        crate::ComplexityMetric,
        crate::NormalizedSpan,
        crate::StructuralFact,
        crate::StructuredPendingRelationship,
        crate::ExtractionResults,
        crate::ExtractionLevel,
        crate::SymbolKind,
        crate::SymbolOptions,
        crate::RelationshipKind,
        crate::IdentifierKind,
        crate::LiteralKind,
        crate::SourceRegionKind,
        crate::ParseDiagnosticKind,
        crate::AnnotationMarker,
        crate::TestRole,
        crate::Visibility,
        crate::LanguageCapabilities,
        crate::CapabilitySnapshot,
        crate::CapabilityFlags,
        crate::CapabilityGap,
        crate::CapabilityKindCoverage,
        crate::CapabilityRow,
        crate::FixtureRef,
        crate::KindCoverage,
        crate::KindCoverageGap,
    >();

    // Constants
    assert!(!crate::EXTRACTION_CONTRACT_VERSION.is_empty());
    assert_eq!(crate::EXTRACTION_IDENTITY_EPOCH, 9);
}
