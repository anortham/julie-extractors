use crate::base::ExtractionResults;
use crate::base::collect_code_structural_facts;
use crate::base::collect_complexity_metrics;
use crate::base::collect_data_structural_facts;
use crate::base::collect_framework_structural_facts;
use crate::base::collect_marker_structural_facts;
use crate::base::collect_source_regions;
use crate::base::collect_sql_structural_facts;
use crate::base::collect_structural_facts;
use crate::base::collect_web_structural_facts;
use crate::base::structural_facts::sort_structural_facts;
use crate::factory::convert_types_map;
use crate::language;
pub use crate::language::LanguageCapabilities;
use anyhow::anyhow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter::Tree;

type ExtractFn = fn(&Tree, &str, &str, &Path) -> Result<ExtractionResults, anyhow::Error>;

#[derive(Debug, Clone, Copy)]
pub struct LanguageRegistryEntry {
    pub language: &'static str,
    pub capabilities: LanguageCapabilities,
    pub extract: ExtractFn,
}

macro_rules! define_structured_full_language_extractors {
    ($(($fn_name:ident, $language:literal, $extractor:path)),+ $(,)?) => {
        $(
            fn $fn_name(
                tree: &Tree,
                file_path: &str,
                content: &str,
                workspace_root: &Path,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = ext.extract_identifiers(tree, &symbols);
                let types = ext.infer_types(&symbols);
                let pending_relationships = ext.base.take_pending_relationships();
                let structured_pending_relationships = ext.base.take_structured_pending_relationships();
                Ok(ExtractionResults {
                    symbols,
                    relationships,
                    pending_relationships,
                    structured_pending_relationships,
                    identifiers,
                    type_argument_usages: ext.base.take_type_argument_usages(),
                    literals: ext.base.take_literals(),
                    source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
                    types: convert_types_map(types, $language),
                    parse_diagnostics: Vec::new(),
                })
            }
        )+
    };
}

macro_rules! define_structured_full_file_extractors {
    ($(($fn_name:ident, $language:literal, $extractor:path)),+ $(,)?) => {
        $(
            fn $fn_name(
                tree: &Tree,
                file_path: &str,
                content: &str,
                workspace_root: &Path,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = ext.extract_identifiers(tree, &symbols);
                let types = ext.infer_types(&symbols);
                let pending_relationships = ext.base.take_pending_relationships();
                let structured_pending_relationships = ext.base.take_structured_pending_relationships();
                Ok(ExtractionResults {
                    symbols,
                    relationships,
                    pending_relationships,
                    structured_pending_relationships,
                    identifiers,
                    type_argument_usages: ext.base.take_type_argument_usages(),
                    literals: ext.base.take_literals(),
                    source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
                    types: convert_types_map(types, $language),
                    parse_diagnostics: Vec::new(),
                })
            }
        )+
    };
}

macro_rules! define_no_pending_extractors {
    ($(($fn_name:ident, $language:literal, $extractor:path)),+ $(,)?) => {
        $(
            fn $fn_name(
                tree: &Tree,
                file_path: &str,
                content: &str,
                workspace_root: &Path,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = ext.extract_identifiers(tree, &symbols);
                let types = ext.infer_types(&symbols);
                Ok(ExtractionResults {
                    symbols,
                    relationships,
                    pending_relationships: Vec::new(),
                    structured_pending_relationships: Vec::new(),
                    identifiers,
                    type_argument_usages: ext.base.take_type_argument_usages(),
                    literals: ext.base.take_literals(),
                    source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
                    types: convert_types_map(types, $language),
                    parse_diagnostics: Vec::new(),
                })
            }
        )+
    };
}

macro_rules! define_relationship_data_extractors {
    ($(($fn_name:ident, $language:literal, $extractor:path)),+ $(,)?) => {
        $(
            fn $fn_name(
                tree: &Tree,
                file_path: &str,
                content: &str,
                workspace_root: &Path,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = ext.extract_identifiers(tree, &symbols);
                Ok(ExtractionResults {
                    symbols,
                    relationships,
                    pending_relationships: Vec::new(),
                    structured_pending_relationships: Vec::new(),
                    identifiers,
                    type_argument_usages: ext.base.take_type_argument_usages(),
                    literals: ext.base.take_literals(),
                    source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
                    types: HashMap::new(),
                    parse_diagnostics: Vec::new(),
                })
            }
        )+
    };
}

define_structured_full_language_extractors![
    (extract_elixir, "elixir", crate::elixir::ElixirExtractor),
    (extract_rust, "rust", crate::rust::RustExtractor),
    (extract_dart, "dart", crate::dart::DartExtractor),
    (extract_go, "go", crate::go::GoExtractor),
    (extract_c, "c", crate::c::CExtractor),
    (extract_zig, "zig", crate::zig::ZigExtractor),
    (extract_vbnet, "vbnet", crate::vbnet::VbNetExtractor),
    (
        extract_gdscript,
        "gdscript",
        crate::gdscript::GDScriptExtractor
    ),
    (extract_java, "java", crate::java::JavaExtractor),
    (extract_csharp, "csharp", crate::csharp::CSharpExtractor),
    (extract_kotlin, "kotlin", crate::kotlin::KotlinExtractor),
    (extract_swift, "swift", crate::swift::SwiftExtractor),
    (extract_php, "php", crate::php::PhpExtractor),
    (extract_scala, "scala", crate::scala::ScalaExtractor),
    (
        extract_typescript,
        "typescript",
        crate::typescript::TypeScriptExtractor
    ),
    (extract_tsx, "tsx", crate::typescript::TypeScriptExtractor),
    (
        extract_javascript,
        "javascript",
        crate::javascript::JavaScriptExtractor
    ),
    (extract_jsx, "jsx", crate::javascript::JavaScriptExtractor),
    (extract_bash, "bash", crate::bash::BashExtractor),
    (
        extract_powershell,
        "powershell",
        crate::powershell::PowerShellExtractor
    ),
    (extract_qml, "qml", crate::qml::QmlExtractor)
];

fn extract_lua(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::lua::LuaExtractor::new(
        "lua".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    let pending_relationships = ext.base.take_pending_relationships();
    let structured_pending_relationships = ext.base.take_structured_pending_relationships();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: HashMap::new(),
        parse_diagnostics: Vec::new(),
    })
}

fn extract_r(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::r::RExtractor::new(
        "r".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    let pending_relationships = ext.base.take_pending_relationships();
    let structured_pending_relationships = ext.base.take_structured_pending_relationships();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: HashMap::new(),
        parse_diagnostics: Vec::new(),
    })
}

define_structured_full_file_extractors![
    (extract_python, "python", crate::python::PythonExtractor),
    (extract_cpp, "cpp", crate::cpp::CppExtractor),
    (extract_ruby, "ruby", crate::ruby::RubyExtractor)
];

define_no_pending_extractors![
    (extract_razor, "razor", crate::razor::RazorExtractor),
    (extract_regex, "regex", crate::regex::RegexExtractor)
];

/// Hand-written HTML extractor entry point. Phase 4b.html graduated HTML out
/// of `define_no_pending_extractors!` so its
/// `extract_structured_pending_relationships` emissions for external
/// `<script src=...>` and `<link href=...>` references reach the canonical
/// extraction results. See `crates/julie-extractors/src/html/relationships.rs`
/// for the shape contract.
fn extract_html(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::html::HTMLExtractor::new(
        "html".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    let types = ext.infer_types(&symbols);
    let structured_pending_relationships =
        ext.extract_structured_pending_relationships(tree, &symbols);
    let pending_relationships = structured_pending_relationships
        .clone()
        .into_iter()
        .map(|pending| pending.into_pending_relationship())
        .collect();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: convert_types_map(types, "html"),
        parse_diagnostics: Vec::new(),
    })
}

/// Hand-written SQL extractor entry point. Phase 3.1 graduated SQL out of
/// `define_no_pending_extractors!` so its `add_structured_pending_relationship`
/// emissions for cross-schema FK targets reach the canonical extraction
/// results. See `crates/julie-extractors/src/sql/relationships.rs` for the
/// FK shape contract.
fn extract_sql(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::sql::SqlExtractor::new(
        "sql".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    let types = ext.infer_types(&symbols);
    let pending_relationships = ext.base.take_pending_relationships();
    let structured_pending_relationships = ext.base.take_structured_pending_relationships();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: convert_types_map(types, "sql"),
        parse_diagnostics: Vec::new(),
    })
}

define_relationship_data_extractors![
    (extract_css, "css", crate::css::CSSExtractor),
    (
        extract_markdown,
        "markdown",
        crate::markdown::MarkdownExtractor
    ),
    (extract_yaml, "yaml", crate::yaml::YamlExtractor)
];

/// TOML extractor (Phase 3.3): hand-written so it can emit domain-aware
/// relationships for Cargo `[dependencies]` and pyproject `[tool.*]`
/// tables. `pending_relationships` stays empty — TOML's references are
/// always file-local; `types` stays empty — TOML has no static type
/// system.
fn extract_toml(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::toml::TomlExtractor::new(
        "toml".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships: Vec::new(),
        structured_pending_relationships: Vec::new(),
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: HashMap::new(),
        parse_diagnostics: Vec::new(),
    })
}

/// Erlang extractor: hand-written because Erlang currently ships the symbol
/// tier only. Relationships, identifiers, and types stay empty until the
/// matching capability rows in `fixtures/extraction/capabilities.json` are
/// raised with fixture evidence.
fn extract_erlang(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::erlang::ErlangExtractor::new(
        "erlang".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    Ok(ExtractionResults {
        symbols,
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        structured_pending_relationships: Vec::new(),
        identifiers: Vec::new(),
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: HashMap::new(),
        parse_diagnostics: Vec::new(),
    })
}

/// JSON extractor (Phase 3.2): hand-written so it can return relationships
/// (concrete + structured pending) for JSON Schema `$ref` shapes.
/// `types` stays empty — JSON has no static type system.
fn extract_json(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::json::JsonExtractor::new(
        "json".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = ext.extract_identifiers(tree, &symbols);
    let pending_relationships = ext.base.take_pending_relationships();
    let structured_pending_relationships = ext.base.take_structured_pending_relationships();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: HashMap::new(),
        parse_diagnostics: Vec::new(),
    })
}

fn extract_vue(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::vue::VueExtractor::new(
        "vue".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(Some(tree));
    let relationships = ext.extract_relationships(Some(tree), &symbols);
    let identifiers = ext.extract_identifiers(&symbols);
    let types = ext.infer_types(&symbols);
    let structured_pending_relationships = ext.extract_structured_pending_relationships(&symbols);
    let pending_relationships = structured_pending_relationships
        .clone()
        .into_iter()
        .map(|pending| pending.into_pending_relationship())
        .collect();
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: convert_types_map(types, "vue"),
        parse_diagnostics: Vec::new(),
    })
}

const EXTRACTORS: &[(&str, ExtractFn)] = &[
    ("rust", extract_rust),
    ("c", extract_c),
    ("cpp", extract_cpp),
    ("go", extract_go),
    ("zig", extract_zig),
    ("typescript", extract_typescript),
    ("tsx", extract_tsx),
    ("javascript", extract_javascript),
    ("jsx", extract_jsx),
    ("html", extract_html),
    ("css", extract_css),
    ("vue", extract_vue),
    ("python", extract_python),
    ("java", extract_java),
    ("csharp", extract_csharp),
    ("vbnet", extract_vbnet),
    ("php", extract_php),
    ("ruby", extract_ruby),
    ("swift", extract_swift),
    ("kotlin", extract_kotlin),
    ("scala", extract_scala),
    ("dart", extract_dart),
    ("elixir", extract_elixir),
    ("erlang", extract_erlang),
    ("lua", extract_lua),
    ("qml", extract_qml),
    ("r", extract_r),
    ("bash", extract_bash),
    ("powershell", extract_powershell),
    ("gdscript", extract_gdscript),
    ("razor", extract_razor),
    ("sql", extract_sql),
    ("regex", extract_regex),
    ("markdown", extract_markdown),
    ("json", extract_json),
    ("toml", extract_toml),
    ("yaml", extract_yaml),
];

fn registry() -> &'static [LanguageRegistryEntry] {
    static REGISTRY: OnceLock<Vec<LanguageRegistryEntry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            language::language_specs()
                .iter()
                .map(|spec| {
                    let extract = EXTRACTORS
                        .iter()
                        .find(|(language, _)| *language == spec.name)
                        .map(|(_, extract)| *extract)
                        .unwrap_or_else(|| panic!("missing extractor for {}", spec.name));
                    LanguageRegistryEntry {
                        language: spec.name,
                        capabilities: spec.capabilities,
                        extract,
                    }
                })
                .collect()
        })
        .as_slice()
}

pub fn registry_entry(language: &str) -> Result<&'static LanguageRegistryEntry, anyhow::Error> {
    registry()
        .iter()
        .find(|entry| entry.language == language)
        .ok_or_else(|| anyhow!("No extractor available for language '{}'", language))
}

pub fn supported_languages() -> Vec<&'static str> {
    language::supported_languages().to_vec()
}

pub fn extract_for_language(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<ExtractionResults, anyhow::Error> {
    let entry = registry_entry(language).map_err(|_| {
        anyhow!(
            "No extractor available for language '{}' (file: {})",
            language,
            file_path
        )
    })?;
    let mut results = (entry.extract)(tree, file_path, content, workspace_root)?;
    results.source_regions =
        collect_source_regions(language, tree, file_path, content, &results.symbols);
    results.structural_facts =
        collect_structural_facts(language, tree, file_path, &results.symbols);
    results
        .structural_facts
        .extend(collect_framework_structural_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
    results
        .structural_facts
        .extend(collect_marker_structural_facts(
            content,
            &results.source_regions,
        ));
    results
        .structural_facts
        .extend(collect_web_structural_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
    results
        .structural_facts
        .extend(collect_code_structural_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
    results
        .structural_facts
        .extend(collect_data_structural_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
    results
        .structural_facts
        .extend(collect_sql_structural_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
    sort_structural_facts(&mut results.structural_facts);
    results.complexity_metrics = match language {
        "sql" => crate::sql::complexity_metrics::collect_complexity_metrics(
            tree,
            content,
            file_path,
            &results.symbols,
        ),
        "regex" => crate::regex::complexity_metrics::collect_complexity_metrics(
            tree,
            file_path,
            &results.symbols,
        ),
        _ => collect_complexity_metrics(language, tree, content, file_path, &results.symbols),
    };
    Ok(results)
}

pub fn capabilities_for_language(language: &str) -> Result<LanguageCapabilities, anyhow::Error> {
    Ok(registry_entry(language)?.capabilities)
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn registry_matches_supported_language_count() {
        assert_eq!(supported_languages().len(), 37);
        assert!(
            capabilities_for_language("rust")
                .unwrap()
                .pending_relationships
        );
        // CSS extracts `@import` directives as `references` relationship edges
        // (see the blessed css entry in fixtures/extraction/capabilities.json:
        // kind_coverage.relationships.supported = ["references"]). This assertion
        // previously hardcoded the opposite and drifted out of sync with the
        // capability golden.
        assert!(capabilities_for_language("css").unwrap().relationships);
    }
}
