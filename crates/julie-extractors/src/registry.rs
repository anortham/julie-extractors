use crate::base::ExtractionLevel;
use crate::base::ExtractionResults;
use crate::base::collect_code_structural_facts;
use crate::base::collect_complexity_metrics;
use crate::base::collect_data_structural_facts;
use crate::base::collect_framework_structural_facts;
use crate::base::collect_marker_structural_facts;
use crate::base::collect_rust_doc_test_facts;
use crate::base::collect_source_regions;
use crate::base::collect_sql_structural_facts;
use crate::base::collect_structural_facts;
use crate::base::collect_web_structural_facts;
use crate::base::structural_facts::sort_structural_facts;
use crate::language;
pub use crate::language::LanguageCapabilities;
use crate::tree_traversal::depth_truncation_diagnostic;
use anyhow::anyhow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter::Tree;

type ExtractFn =
    fn(&Tree, &str, &str, &Path, ExtractionLevel) -> Result<ExtractionResults, anyhow::Error>;

#[derive(Debug, Clone, Copy)]
pub struct LanguageRegistryEntry {
    pub language: &'static str,
    pub capabilities: LanguageCapabilities,
    pub extract: ExtractFn,
}

/// Convert a raw type map from `infer_types()` into the richer `TypeInfo` structure.
///
/// Legacy inferred values sometimes carry raw source text instead of a type
/// name. Values that can never verbatim-match a type symbol (whitespace, a
/// comma, a trailing `<`, or a `>` without `<`) are dropped; everything else
/// is kept exactly as-is because these rows predate the base-name contract.
pub(crate) fn convert_types_map(
    types: HashMap<String, String>,
    language: &str,
) -> HashMap<String, crate::base::TypeInfo> {
    types
        .into_iter()
        .filter(|(_, type_string)| is_bindable_type_name(type_string))
        .map(|(symbol_id, type_string)| {
            (
                symbol_id.clone(),
                crate::base::TypeInfo {
                    symbol_id,
                    resolved_type: type_string,
                    generic_params: None,
                    constraints: None,
                    is_inferred: true,
                    language: language.to_string(),
                    metadata: None,
                },
            )
        })
        .collect()
}

fn is_bindable_type_name(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) || value.contains(',') || value.ends_with('<') {
        return false;
    }
    !value.contains('>') || value.contains('<')
}

/// Every extractor that infers types must also keep the `TypeInfo` rows its
/// base recorded during extraction; a recorded row wins over an inferred one.
fn types_with_base_info(
    inferred: HashMap<String, String>,
    language: &str,
    base: &crate::base::BaseExtractor,
) -> HashMap<String, crate::base::TypeInfo> {
    let mut types = convert_types_map(inferred, language);
    types.extend(base.type_info.clone());
    types
}

macro_rules! define_structured_full_language_extractors {
    ($(($fn_name:ident, $language:literal, $extractor:path)),+ $(,)?) => {
        $(
            fn $fn_name(
                tree: &Tree,
                file_path: &str,
                content: &str,
                workspace_root: &Path,
                level: ExtractionLevel,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = if level.includes_references() {
                    ext.extract_identifiers(tree, &symbols)
                } else {
                    Vec::new()
                };
                let types = types_with_base_info(ext.infer_types(&symbols), $language, &ext.base);
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
                    types,
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
                level: ExtractionLevel,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = if level.includes_references() {
                    ext.extract_identifiers(tree, &symbols)
                } else {
                    Vec::new()
                };
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
                    types: types_with_base_info(types, $language, &ext.base),
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
                level: ExtractionLevel,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = if level.includes_references() {
                    ext.extract_identifiers(tree, &symbols)
                } else {
                    Vec::new()
                };
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
                    types: types_with_base_info(types, $language, &ext.base),
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
                level: ExtractionLevel,
            ) -> Result<ExtractionResults, anyhow::Error> {
                let mut ext = <$extractor>::new(
                    $language.to_string(),
                    file_path.to_string(),
                    content.to_string(),
                    workspace_root,
                );
                let symbols = ext.extract_symbols(tree);
                let relationships = ext.extract_relationships(tree, &symbols);
                let identifiers = if level.includes_references() {
                    ext.extract_identifiers(tree, &symbols)
                } else {
                    Vec::new()
                };
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
    (extract_qml, "qml", crate::qml::QmlExtractor),
    (extract_fsharp, "fsharp", crate::fsharp::FSharpExtractor)
];

fn extract_lua(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::lua::LuaExtractor::new(
        "lua".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
    let types = types_with_base_info(ext.infer_types(&symbols), "lua", &ext.base);
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
        types,
        parse_diagnostics: Vec::new(),
    })
}

fn extract_r(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::r::RExtractor::new(
        "r".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
    let types = types_with_base_info(ext.infer_types(&symbols), "r", &ext.base);
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
        types,
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
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::html::HTMLExtractor::new(
        "html".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
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
        types: types_with_base_info(types, "html", &ext.base),
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
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::sql::SqlExtractor::new(
        "sql".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
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
        types: types_with_base_info(types, "sql", &ext.base),
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
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::toml::TomlExtractor::new(
        "toml".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
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

/// Erlang extractor: hand-written so it can return structured pending
/// relationships for remote calls, `-behaviour`, `-include` and `-import`.
fn extract_erlang(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::erlang::ErlangExtractor::new(
        "erlang".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
    let types = ext.infer_types(&symbols);
    Ok(ExtractionResults {
        symbols,
        relationships,
        pending_relationships: ext.get_pending_relationships(),
        structured_pending_relationships: ext.get_structured_pending_relationships(),
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        types: types_with_base_info(types, "erlang", &ext.base),
        parse_diagnostics: ext.parse_diagnostics(),
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
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::json::JsonExtractor::new(
        "json".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let relationships = ext.extract_relationships(tree, &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
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

/// XML extractor: hand-written because XML ships the data tier (symbols plus
/// QName attribute-reference identifiers). Relationships and types stay empty —
/// resolving a QName reference to its declaration needs namespace resolution,
/// which v1 does not perform.
fn extract_xml(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::xml::XmlExtractor::new(
        "xml".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
    Ok(ExtractionResults {
        symbols,
        relationships: Vec::new(),
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

fn extract_qmldir(
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::qmldir::QmldirExtractor::new(
        "qmldir".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(tree);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(tree, &symbols)
    } else {
        Vec::new()
    };
    Ok(ExtractionResults {
        symbols,
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        structured_pending_relationships: Vec::new(),
        identifiers,
        type_argument_usages: ext.base.take_type_argument_usages(),
        literals: ext.base.take_literals(),
        source_regions: Vec::new(),
        structural_facts: ext.take_structural_facts(),
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
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let mut ext = crate::vue::VueExtractor::new(
        "vue".to_string(),
        file_path.to_string(),
        content.to_string(),
        workspace_root,
    );
    let symbols = ext.extract_symbols(Some(tree));
    let relationships = ext.extract_relationships(Some(tree), &symbols);
    let identifiers = if level.includes_references() {
        ext.extract_identifiers(&symbols)
    } else {
        Vec::new()
    };
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
        types: types_with_base_info(types, "vue", &ext.base),
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
    ("fsharp", extract_fsharp),
    ("lua", extract_lua),
    ("qml", extract_qml),
    ("qmldir", extract_qmldir),
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
    ("xml", extract_xml),
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
    extract_for_language_at(
        language,
        tree,
        file_path,
        content,
        workspace_root,
        ExtractionLevel::Full,
    )
}

pub fn extract_for_language_at(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    workspace_root: &Path,
    level: ExtractionLevel,
) -> Result<ExtractionResults, anyhow::Error> {
    let entry = registry_entry(language).map_err(|_| {
        anyhow!(
            "No extractor available for language '{}' (file: {})",
            language,
            file_path
        )
    })?;
    let mut results = (entry.extract)(tree, file_path, content, workspace_root, level)?;
    if level.includes_references() {
        let extractor_structural_facts = std::mem::take(&mut results.structural_facts);
        results.source_regions =
            collect_source_regions(language, tree, file_path, content, &results.symbols);
        results.structural_facts =
            collect_structural_facts(language, tree, file_path, &results.symbols);
        results.structural_facts.extend(collect_rust_doc_test_facts(
            language,
            tree,
            file_path,
            content,
            &results.symbols,
        ));
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
        results.structural_facts.extend(extractor_structural_facts);
        sort_structural_facts(&mut results.structural_facts);
    }
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
    if let Some(diagnostic) = depth_truncation_diagnostic(tree.root_node()) {
        results.parse_diagnostics.push(diagnostic);
    }
    if !level.includes_references() {
        results.strip_to_symbols_level();
    }
    Ok(results)
}

pub fn capabilities_for_language(language: &str) -> Result<LanguageCapabilities, anyhow::Error> {
    Ok(registry_entry(language)?.capabilities)
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::base::{BaseExtractor, TypeInfo};

    #[test]
    fn types_with_base_info_keep_inferred_and_extractor_recorded_entries() {
        let mut base = BaseExtractor::new(
            "fsharp".to_string(),
            "src/lib.fs".to_string(),
            String::new(),
            Path::new("/workspace"),
        );
        base.type_info.insert(
            "recorded".to_string(),
            TypeInfo {
                symbol_id: "recorded".to_string(),
                resolved_type: "int".to_string(),
                generic_params: None,
                constraints: None,
                is_inferred: false,
                language: "fsharp".to_string(),
                metadata: None,
            },
        );
        let inferred = HashMap::from([
            ("inferred".to_string(), "string".to_string()),
            ("recorded".to_string(), "obj".to_string()),
        ]);

        let types = types_with_base_info(inferred, "fsharp", &base);

        assert_eq!(types.len(), 2);
        assert_eq!(types["inferred"].resolved_type, "string");
        assert_eq!(types["recorded"].resolved_type, "int");
        assert!(!types["recorded"].is_inferred);
    }

    #[test]
    fn registry_matches_supported_language_count() {
        assert_eq!(supported_languages().len(), 40);
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
