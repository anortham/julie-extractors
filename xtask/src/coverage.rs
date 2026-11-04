use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const MANIFEST_PATH: &str = "fixtures/real-world/coverage-manifest.json";
const DOMAINS: &[&str] = &[
    "symbols",
    "relationships",
    "identifiers",
    "identifier_metadata",
    "body_spans",
    "structural_facts",
    "complexity_metrics",
    "annotations",
    "doc_comments",
    "literals",
    "source_regions",
    "test_detection",
    "pending_relationships",
    "types",
    "type_argument_usages",
];

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageManifest {
    pub format_version: u32,
    pub languages: Vec<CoverageSample>,
    #[serde(default)]
    pub controls: Vec<CoverageControl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageSample {
    pub language: String,
    pub source_path: String,
    pub source_sha256: String,
    pub provenance: SourceProvenance,
    pub expected_domains: Vec<String>,
    pub expected_facts: Vec<FactAssertion>,
    pub negative_controls: Vec<FactAssertion>,
    pub expected_parse_diagnostics: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenance {
    pub project: String,
    pub source_url: String,
    pub revision: String,
    pub source_file: String,
    pub license: String,
    pub license_evidence_url: String,
    pub license_evidence_files: Vec<LicenseEvidenceFile>,
    #[serde(default)]
    pub source_selection: Option<SourceSelection>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseEvidenceFile {
    pub kind: String,
    pub path: String,
    pub sha256: String,
    pub source_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSelection {
    pub kind: String,
    pub container_path: String,
    pub start_byte: u64,
    pub end_byte: u64,
    pub line_start: u64,
    pub line_end: u64,
    pub container_sha256: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageControl {
    pub name: String,
    pub language: String,
    pub source_path: String,
    pub source_sha256: String,
    pub authored_by: String,
    pub license: String,
    pub license_evidence_files: Vec<LicenseEvidenceFile>,
    pub expected_domains: Vec<String>,
    pub expected_facts: Vec<FactAssertion>,
    pub negative_controls: Vec<FactAssertion>,
    pub expected_parse_diagnostics: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactAssertion {
    pub domain: String,
    pub table: String,
    pub field: String,
    pub value: String,
    pub min_count: u64,
    pub max_count: Option<u64>,
    pub expected_start_byte: Option<u64>,
    #[serde(default)]
    pub fact_language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactObservation {
    pub domain: String,
    pub table: String,
    pub field: String,
    pub value: String,
    pub actual_count: u64,
}

#[derive(Debug)]
pub struct CoverageError(String);

#[derive(Debug)]
pub struct CoverageUsageError(String);

impl Display for CoverageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CoverageError {}

impl Display for CoverageUsageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<std::io::Error> for CoverageError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<rusqlite::Error> for CoverageError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for CoverageError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveragePlan {
    pub language: Option<String>,
    pub all_languages: bool,
    pub binary: PathBuf,
    pub manifest: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageResult {
    pub language: String,
    pub case: String,
    pub file_name: String,
    pub files_scanned: u64,
    pub files_failed: u64,
    pub files_unsupported: u64,
    pub parse_diagnostics: u64,
    pub facts: Vec<FactObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScanReportHealth {
    files_scanned: u64,
    files_failed: u64,
    files_unsupported: u64,
    parse_diagnostics: u64,
}

impl CoveragePlan {
    pub fn from_args(args: &[String]) -> Result<Self, CoverageUsageError> {
        let mut language = None;
        let mut all_languages = false;
        let mut binary = None;
        let mut manifest = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--language" => {
                    if language.is_some() {
                        return Err(CoverageUsageError("duplicate --language".to_string()));
                    }
                    index += 1;
                    language = Some(required_value(args, index, "--language")?.to_string());
                }
                "--all" => {
                    if all_languages {
                        return Err(CoverageUsageError("duplicate --all".to_string()));
                    }
                    all_languages = true;
                }
                "--binary" => {
                    if binary.is_some() {
                        return Err(CoverageUsageError("duplicate --binary".to_string()));
                    }
                    index += 1;
                    binary = Some(PathBuf::from(required_value(args, index, "--binary")?));
                }
                "--manifest" => {
                    if manifest.is_some() {
                        return Err(CoverageUsageError("duplicate --manifest".to_string()));
                    }
                    index += 1;
                    manifest = Some(PathBuf::from(required_value(args, index, "--manifest")?));
                }
                value => {
                    return Err(CoverageUsageError(format!(
                        "unexpected coverage argument {value}"
                    )));
                }
            }
            index += 1;
        }

        if language.is_some() == all_languages {
            return Err(CoverageUsageError(
                "choose exactly one of --language <language> or --all".to_string(),
            ));
        }

        Ok(Self {
            language,
            all_languages,
            binary: binary.ok_or_else(|| CoverageUsageError("missing --binary".to_string()))?,
            manifest,
        })
    }
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    let plan = match CoveragePlan::from_args(args) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match run(plan) {
        Ok(results) => {
            for result in results {
                let facts = result
                    .facts
                    .iter()
                    .map(|fact| {
                        format!(
                            "{}.{}={}({})",
                            fact.table, fact.field, fact.value, fact.actual_count
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "{} [{}]: files_scanned={} files_failed={} files_unsupported={} parse_diagnostics={} facts=[{}]",
                    result.language,
                    result.case,
                    result.files_scanned,
                    result.files_failed,
                    result.files_unsupported,
                    result.parse_diagnostics,
                    facts
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

pub fn run(plan: CoveragePlan) -> Result<Vec<LanguageResult>, CoverageError> {
    let repo_root = repo_root();
    let manifest_path = resolve_repo_path(
        &repo_root,
        plan.manifest.as_deref().unwrap_or(Path::new(MANIFEST_PATH)),
    );
    let manifest = load_manifest(&manifest_path)?;
    validate_manifest_shape(&manifest)?;
    let registry_languages = registry_languages(&repo_root)?;
    let selected = select_samples(&manifest, &registry_languages, &plan)?;
    for control in &manifest.controls {
        if !registry_languages.contains(&control.language) {
            return Err(CoverageError(format!(
                "control {} references unknown language {}",
                control.name, control.language
            )));
        }
    }
    let binary = fs::canonicalize(&plan.binary).map_err(|error| {
        CoverageError(format!(
            "failed to resolve extractor binary {}: {error}",
            plan.binary.display()
        ))
    })?;
    if !binary.is_file() {
        return Err(CoverageError(format!(
            "extractor binary is not a file: {}",
            binary.display()
        )));
    }

    let mut results = selected
        .into_iter()
        .map(|sample| run_sample(&repo_root, &binary, SampleInput::sample(sample)))
        .collect::<Result<Vec<_>, _>>()?;
    results.extend(
        manifest
            .controls
            .iter()
            .filter(|control| {
                plan.all_languages || plan.language.as_deref() == Some(control.language.as_str())
            })
            .map(|control| run_sample(&repo_root, &binary, SampleInput::control(control)))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(results)
}

pub fn load_manifest(path: &Path) -> Result<CoverageManifest, CoverageError> {
    let content = fs::read_to_string(path).map_err(|error| {
        CoverageError(format!(
            "failed to read coverage manifest {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&content).map_err(|error| {
        CoverageError(format!(
            "invalid coverage manifest {}: {error}",
            path.display()
        ))
    })
}

pub fn validate_manifest_shape(manifest: &CoverageManifest) -> Result<(), CoverageError> {
    if manifest.format_version != 1 {
        return Err(CoverageError(format!(
            "unsupported coverage manifest format version {}",
            manifest.format_version
        )));
    }

    let mut languages = BTreeSet::new();
    for sample in &manifest.languages {
        if sample.language.trim().is_empty() || !languages.insert(sample.language.as_str()) {
            return Err(CoverageError(format!(
                "invalid or duplicate coverage language {}",
                sample.language
            )));
        }
        validate_relative_path(&sample.source_path)?;
        validate_sha256(&sample.source_sha256)?;
        validate_provenance(&sample.provenance)?;
        if sample.expected_domains.is_empty() {
            return Err(CoverageError(format!(
                "{} has no expected domains",
                sample.language
            )));
        }
        let mut domains = BTreeSet::new();
        for domain in &sample.expected_domains {
            if !DOMAINS.contains(&domain.as_str()) || !domains.insert(domain) {
                return Err(CoverageError(format!(
                    "{} has invalid or duplicate expected domain {}",
                    sample.language, domain
                )));
            }
        }
        if sample.expected_facts.is_empty() {
            return Err(CoverageError(format!(
                "{} has no positive SQLite fact assertions",
                sample.language
            )));
        }
        for assertion in &sample.expected_facts {
            validate_assertion(assertion, true)?;
        }
        validate_domain_proofs(
            &sample.language,
            &sample.expected_domains,
            &sample.expected_facts,
        )?;
        for assertion in &sample.negative_controls {
            validate_assertion(assertion, false)?;
        }
    }

    let mut control_names = BTreeSet::new();
    for control in &manifest.controls {
        if control.name.trim().is_empty()
            || control.language.trim().is_empty()
            || control.authored_by.trim().is_empty()
            || control.license.trim().is_empty()
            || !control_names.insert(control.name.as_str())
        {
            return Err(CoverageError(format!(
                "invalid or duplicate locally authored control {}",
                control.name
            )));
        }
        validate_relative_path(&control.source_path)?;
        validate_sha256(&control.source_sha256)?;
        validate_license_evidence_files(&control.name, &control.license_evidence_files, None)?;
        if control.expected_domains.is_empty() {
            return Err(CoverageError(format!(
                "locally authored control {} has no expected domains",
                control.name
            )));
        }
        let mut domains = BTreeSet::new();
        for domain in &control.expected_domains {
            if !DOMAINS.contains(&domain.as_str()) || !domains.insert(domain) {
                return Err(CoverageError(format!(
                    "locally authored control {} has invalid or duplicate expected domain {}",
                    control.name, domain
                )));
            }
        }
        if control.expected_facts.is_empty() {
            return Err(CoverageError(format!(
                "locally authored control {} has no positive SQLite fact assertions",
                control.name
            )));
        }
        for assertion in &control.expected_facts {
            validate_assertion(assertion, true)?;
        }
        validate_domain_proofs(
            &control.name,
            &control.expected_domains,
            &control.expected_facts,
        )?;
        for assertion in &control.negative_controls {
            validate_assertion(assertion, false)?;
        }
    }

    Ok(())
}

pub fn validate_source_hash(path: &Path, expected: &str) -> Result<(), CoverageError> {
    validate_sha256(expected)?;
    let bytes = fs::read(path).map_err(|error| {
        CoverageError(format!(
            "failed to read corpus source {}: {error}",
            path.display()
        ))
    })?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(CoverageError(format!(
            "content hash mismatch for {}: expected {expected}, found {actual}",
            path.display()
        )));
    }
    Ok(())
}

pub fn verify_fact_assertions(
    connection: &Connection,
    source_path: &str,
    language: &str,
    assertions: &[FactAssertion],
) -> Result<Vec<FactObservation>, CoverageError> {
    let mut observations = Vec::with_capacity(assertions.len());
    for assertion in assertions {
        validate_assertion(assertion, assertion.min_count > 0)?;
        let (from, predicates) = fact_query(assertion)?;
        let position_expression = fact_position_expression(&assertion.table);
        let query = match (assertion.expected_start_byte, position_expression) {
            (Some(_), Some(position)) => {
                format!("SELECT COUNT(*) FROM {from} WHERE {predicates} AND {position} = ?4")
            }
            (Some(_), None) => {
                return Err(CoverageError(format!(
                    "fact {}.{} does not carry a source start byte",
                    assertion.table, assertion.field
                )));
            }
            (None, _) => format!("SELECT COUNT(*) FROM {from} WHERE {predicates}"),
        };
        let actual_count: i64 = if let Some(start_byte) = assertion.expected_start_byte {
            let start_byte = i64::try_from(start_byte).map_err(|_| {
                CoverageError(format!(
                    "expected byte offset {} exceeds SQLite's signed integer range",
                    start_byte
                ))
            })?;
            let language = assertion.fact_language.as_deref().unwrap_or(language);
            connection.query_row(
                &query,
                rusqlite::params![source_path, language, assertion.value, start_byte],
                |row| row.get(0),
            )?
        } else {
            let language = assertion.fact_language.as_deref().unwrap_or(language);
            connection.query_row(
                &query,
                rusqlite::params![source_path, language, assertion.value],
                |row| row.get(0),
            )?
        };
        let actual_count = count_to_u64(actual_count)?;
        if actual_count < assertion.min_count
            || assertion
                .max_count
                .is_some_and(|maximum| actual_count > maximum)
        {
            let upper = assertion
                .max_count
                .map(|maximum| maximum.to_string())
                .unwrap_or_else(|| "unbounded".to_string());
            return Err(CoverageError(format!(
                "{} {}={:?} for {} {}: expected {}..{}, found {}",
                assertion.table,
                assertion.field,
                assertion.value,
                language,
                source_path,
                assertion.min_count,
                upper,
                actual_count
            )));
        }
        observations.push(FactObservation {
            domain: assertion.domain.clone(),
            table: assertion.table.clone(),
            field: assertion.field.clone(),
            value: assertion.value.clone(),
            actual_count,
        });
    }
    Ok(observations)
}

fn fact_query(assertion: &FactAssertion) -> Result<(String, String), CoverageError> {
    let field = assertion.field.as_str();
    let (from, allowed_fields, value_expression, path_expression, language_expression) =
        match assertion.table.as_str() {
            "files" => (
                "files",
                &["language", "status", "content_hash"][..],
                format!("files.{field}"),
                "files.path",
                "files.language",
            ),
            "symbols" => (
                "symbols",
                &[
                    "name",
                    "kind",
                    "language",
                    "visibility",
                    "doc_comment",
                    "body_start_byte",
                    "body_end_byte",
                    "body_hash",
                    "is_test",
                    "test_container",
                    "test_lifecycle",
                ][..],
                format!("symbols.{field}"),
                "symbols.path",
                "symbols.language",
            ),
            "symbol_annotations" => (
                "symbol_annotations JOIN symbols ON symbols.symbol_id = symbol_annotations.symbol_id",
                &["annotation", "annotation_key", "carrier"][..],
                format!("symbol_annotations.{field}"),
                "symbols.path",
                "symbols.language",
            ),
            "reference_sites" => (
                "reference_sites",
                &["provenance", "is_exact"][..],
                format!("reference_sites.{field}"),
                "reference_sites.path",
                "reference_sites.language",
            ),
            "identifiers" => (
                "identifiers",
                &["name", "kind", "language", "confidence", "metadata_json"][..],
                format!("identifiers.{field}"),
                "identifiers.path",
                "identifiers.language",
            ),
            "relationships" => (
                "relationships",
                &["kind", "path", "language", "confidence"][..],
                format!("relationships.{field}"),
                "relationships.path",
                "relationships.language",
            ),
            "pending_relationships" => (
                "pending_relationships",
                &[
                    "kind",
                    "target_display_name",
                    "target_terminal_name",
                    "target_receiver",
                    "language",
                ][..],
                format!("pending_relationships.{field}"),
                "pending_relationships.path",
                "pending_relationships.language",
            ),
            "type_facts" => (
                "type_facts JOIN symbols ON symbols.symbol_id = type_facts.symbol_id",
                &["resolved_type", "is_inferred", "language"][..],
                format!("type_facts.{field}"),
                "symbols.path",
                "type_facts.language",
            ),
            "type_argument_usages" => (
                "type_argument_usages",
                &["language", "path"][..],
                format!("type_argument_usages.{field}"),
                "type_argument_usages.path",
                "type_argument_usages.language",
            ),
            "type_arguments" => (
                "type_arguments JOIN type_argument_usages ON type_argument_usages.usage_id = type_arguments.usage_id",
                &["ordinal", "type_name"][..],
                format!("type_arguments.{field}"),
                "type_argument_usages.path",
                "type_argument_usages.language",
            ),
            "literals" => (
                "literals",
                &[
                    "literal_text",
                    "kind",
                    "carrier",
                    "arg_position",
                    "language",
                ][..],
                format!("literals.{field}"),
                "literals.path",
                "literals.language",
            ),
            "source_regions" => (
                "source_regions",
                &["kind", "language"][..],
                format!("source_regions.{field}"),
                "source_regions.path",
                "source_regions.language",
            ),
            "structural_facts" => (
                "structural_facts",
                &["pattern_id", "capture_name", "node_kind", "language"][..],
                format!("structural_facts.{field}"),
                "structural_facts.path",
                "structural_facts.language",
            ),
            "complexity_metrics" => (
                "complexity_metrics",
                &["scope", "algorithm_id", "language"][..],
                format!("complexity_metrics.{field}"),
                "complexity_metrics.path",
                "complexity_metrics.language",
            ),
            "parse_diagnostics" => (
                "parse_diagnostics",
                &["kind", "language"][..],
                format!("parse_diagnostics.{field}"),
                "parse_diagnostics.path",
                "parse_diagnostics.language",
            ),
            _ => {
                return Err(CoverageError(format!(
                    "unsupported fact table {}",
                    assertion.table
                )));
            }
        };

    if !allowed_fields.contains(&field) {
        return Err(CoverageError(format!(
            "unsupported fact field {} for table {}",
            field, assertion.table
        )));
    }

    if assertion.domain != domain_for_table(assertion.table.as_str(), field)? {
        return Err(CoverageError(format!(
            "fact {}.{} belongs to domain {}, not {}",
            assertion.table,
            field,
            domain_for_table(assertion.table.as_str(), field)?,
            assertion.domain
        )));
    }

    Ok((
        from.to_string(),
        format!(
            "{} = ?1 AND {} = ?2 AND {} = ?3",
            path_expression, language_expression, value_expression
        ),
    ))
}

fn fact_position_expression(table: &str) -> Option<&'static str> {
    match table {
        "symbols" => Some("symbols.start_byte"),
        "symbol_annotations" => Some("symbols.start_byte"),
        "identifiers" => Some("identifiers.start_byte"),
        "relationships" => Some("relationships.start_byte"),
        "pending_relationships" => Some("pending_relationships.start_byte"),
        "literals" => Some("literals.start_byte"),
        "source_regions" => Some("source_regions.start_byte"),
        "structural_facts" => Some("structural_facts.start_byte"),
        "complexity_metrics" => Some("complexity_metrics.start_byte"),
        "parse_diagnostics" => Some("parse_diagnostics.start_byte"),
        _ => None,
    }
}

fn domain_for_table(table: &str, field: &str) -> Result<&'static str, CoverageError> {
    match table {
        "symbols" => match field {
            "body_start_byte" | "body_end_byte" | "body_hash" => Ok("body_spans"),
            "doc_comment" => Ok("doc_comments"),
            "is_test" | "test_container" | "test_lifecycle" => Ok("test_detection"),
            _ => Ok("symbols"),
        },
        "symbol_annotations" => Ok("annotations"),
        "identifiers" if field == "metadata_json" => Ok("identifier_metadata"),
        "identifiers" => Ok("identifiers"),
        "relationships" => Ok("relationships"),
        "pending_relationships" => Ok("pending_relationships"),
        "type_facts" => Ok("types"),
        "type_argument_usages" | "type_arguments" => Ok("type_argument_usages"),
        "literals" => Ok("literals"),
        "source_regions" => Ok("source_regions"),
        "structural_facts" => Ok("structural_facts"),
        "complexity_metrics" => Ok("complexity_metrics"),
        _ => Err(CoverageError(format!(
            "table {} cannot prove a language coverage domain",
            table
        ))),
    }
}

fn validate_domain_proofs(
    name: &str,
    expected_domains: &[String],
    expected_facts: &[FactAssertion],
) -> Result<(), CoverageError> {
    let expected: BTreeSet<_> = expected_domains.iter().map(String::as_str).collect();
    let proved: BTreeSet<_> = expected_facts
        .iter()
        .map(|fact| fact.domain.as_str())
        .collect();
    if expected.is_empty() || expected != proved {
        return Err(CoverageError(format!(
            "{} expected domains and positive fact domain proofs do not match: declared [{}], proof [{}]",
            name,
            expected.iter().copied().collect::<Vec<_>>().join(", "),
            proved.iter().copied().collect::<Vec<_>>().join(", ")
        )));
    }
    if expected.iter().any(|domain| !DOMAINS.contains(domain)) {
        return Err(CoverageError(format!(
            "{} has an unknown expected domain",
            name
        )));
    }
    Ok(())
}

fn validate_assertion(assertion: &FactAssertion, positive: bool) -> Result<(), CoverageError> {
    if assertion.value.is_empty() {
        return Err(CoverageError(
            "fact assertion values cannot be empty".to_string(),
        ));
    }
    if assertion
        .max_count
        .is_some_and(|maximum| maximum < assertion.min_count)
    {
        return Err(CoverageError(format!(
            "invalid count range for {} {}={:?}",
            assertion.table, assertion.field, assertion.value
        )));
    }
    if positive && assertion.min_count == 0 {
        return Err(CoverageError(format!(
            "positive fact {} {}={:?} must require at least one row",
            assertion.table, assertion.field, assertion.value
        )));
    }
    if !positive && (assertion.min_count != 0 || assertion.max_count != Some(0)) {
        return Err(CoverageError(format!(
            "negative control {} {}={:?} must require exactly zero rows",
            assertion.table, assertion.field, assertion.value
        )));
    }
    if assertion
        .fact_language
        .as_deref()
        .is_some_and(|language| language.trim().is_empty())
    {
        return Err(CoverageError(
            "fact assertion language cannot be empty".to_string(),
        ));
    }
    fact_query(assertion)?;
    Ok(())
}

fn validate_provenance(provenance: &SourceProvenance) -> Result<(), CoverageError> {
    for (label, value) in [
        ("project", provenance.project.as_str()),
        ("source URL", provenance.source_url.as_str()),
        ("source file", provenance.source_file.as_str()),
        ("license", provenance.license.as_str()),
        (
            "license evidence URL",
            provenance.license_evidence_url.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(CoverageError(format!(
                "source provenance is missing {}",
                label
            )));
        }
    }
    if !(12..=64).contains(&provenance.revision.len())
        || !provenance
            .revision
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(CoverageError(format!(
            "{} has no immutable hexadecimal source revision",
            provenance.project
        )));
    }
    if !provenance.source_url.starts_with("https://")
        || !provenance.license_evidence_url.starts_with("https://")
    {
        return Err(CoverageError(format!(
            "{} provenance URLs must use HTTPS",
            provenance.project
        )));
    }
    if !url_contains_revision(&provenance.source_url, &provenance.revision)
        || !url_contains_revision(&provenance.license_evidence_url, &provenance.revision)
    {
        return Err(CoverageError(format!(
            "{} source and license evidence URLs must bind revision {}",
            provenance.project, provenance.revision
        )));
    }
    validate_license_evidence_files(
        &provenance.project,
        &provenance.license_evidence_files,
        Some((&provenance.revision, &provenance.license_evidence_url)),
    )?;
    if let Some(selection) = &provenance.source_selection {
        if selection.kind != "byte_range"
            || selection.container_path.trim().is_empty()
            || selection.end_byte <= selection.start_byte
            || selection.line_start == 0
            || selection.line_end < selection.line_start
            || selection.description.trim().is_empty()
        {
            return Err(CoverageError(format!(
                "{} has an invalid source selection",
                provenance.project
            )));
        }
        validate_relative_path(&selection.container_path)?;
        validate_sha256(&selection.container_sha256)?;
    }
    Ok(())
}

fn validate_license_evidence_files(
    name: &str,
    evidence_files: &[LicenseEvidenceFile],
    revision_binding: Option<(&str, &str)>,
) -> Result<(), CoverageError> {
    if evidence_files.is_empty() {
        return Err(CoverageError(format!(
            "{} has no offline license evidence files",
            name
        )));
    }
    let mut has_license = false;
    let mut has_primary_url = revision_binding.is_none();
    let mut paths = BTreeSet::new();
    for evidence in evidence_files {
        validate_relative_path(&evidence.path)?;
        validate_sha256(&evidence.sha256)?;
        if !matches!(evidence.kind.as_str(), "license" | "notice" | "scope") {
            return Err(CoverageError(format!(
                "{} has invalid license evidence kind {}",
                name, evidence.kind
            )));
        }
        if !evidence.source_url.starts_with("https://") {
            return Err(CoverageError(format!(
                "{} license evidence URLs must use HTTPS",
                name
            )));
        }
        if let Some((revision, primary_url)) = revision_binding {
            if !url_contains_revision(&evidence.source_url, revision) {
                return Err(CoverageError(format!(
                    "{} license evidence URL does not bind revision {}",
                    name, revision
                )));
            }
            has_primary_url |= evidence.source_url == primary_url;
        }
        has_license |= evidence.kind == "license";
        paths.insert(evidence.path.as_str());
    }
    if !has_license {
        return Err(CoverageError(format!("{} has no local license text", name)));
    }
    if !has_primary_url {
        return Err(CoverageError(format!(
            "{} primary license evidence URL has no offline file mapping",
            name
        )));
    }
    if paths.len() != evidence_files.len() {
        return Err(CoverageError(format!(
            "{} has duplicate offline license evidence paths",
            name
        )));
    }
    Ok(())
}

fn url_contains_revision(url: &str, revision: &str) -> bool {
    url.match_indices(revision).any(|(start, matched)| {
        let end = start + matched.len();
        let before_is_boundary = url[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_ascii_alphanumeric());
        let after_is_boundary = url[end..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_ascii_alphanumeric());
        before_is_boundary && after_is_boundary
    })
}

fn validate_relative_path(value: &str) -> Result<(), CoverageError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CoverageError(format!(
            "corpus path must be a nonempty relative path without parent traversal: {}",
            value
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), CoverageError> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(CoverageError(format!("invalid SHA-256 digest {}", value)));
    }
    Ok(())
}

fn select_samples<'a>(
    manifest: &'a CoverageManifest,
    registry_languages: &BTreeSet<String>,
    plan: &CoveragePlan,
) -> Result<Vec<&'a CoverageSample>, CoverageError> {
    let manifest_languages: BTreeSet<_> = manifest
        .languages
        .iter()
        .map(|sample| sample.language.clone())
        .collect();
    let unknown: Vec<_> = manifest_languages
        .difference(registry_languages)
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(CoverageError(format!(
            "coverage manifest contains unknown language(s): {}",
            unknown.join(", ")
        )));
    }

    if let Some(language) = &plan.language {
        if !registry_languages.contains(language) {
            return Err(CoverageError(format!("unknown language {}", language)));
        }
        let sample = manifest
            .languages
            .iter()
            .find(|sample| sample.language == *language)
            .ok_or_else(|| {
                CoverageError(format!("coverage manifest has no sample for {}", language))
            })?;
        return Ok(vec![sample]);
    }

    let missing: Vec<_> = registry_languages
        .difference(&manifest_languages)
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(CoverageError(format!(
            "coverage manifest is incomplete: missing {} language sample(s): {}",
            missing.len(),
            missing.join(", ")
        )));
    }
    Ok(manifest.languages.iter().collect())
}

fn registry_languages(repo_root: &Path) -> Result<BTreeSet<String>, CoverageError> {
    let path = repo_root.join("fixtures/extraction/capabilities.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).map_err(|error| {
        CoverageError(format!(
            "failed to read language registry {}: {error}",
            path.display()
        ))
    })?)?;
    let rows = value
        .get("languages")
        .and_then(Value::as_array)
        .ok_or_else(|| CoverageError("capability manifest has no languages array".to_string()))?;
    rows.iter()
        .map(|row| {
            row.get("language")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    CoverageError("language registry row has no language string".to_string())
                })
        })
        .collect()
}

struct SampleInput<'a> {
    language: &'a str,
    case: &'a str,
    source_path: &'a str,
    source_sha256: &'a str,
    expected_parse_diagnostics: u64,
    expected_facts: &'a [FactAssertion],
    negative_controls: &'a [FactAssertion],
    license_evidence_files: &'a [LicenseEvidenceFile],
    source_selection: Option<&'a SourceSelection>,
}

impl<'a> SampleInput<'a> {
    fn sample(sample: &'a CoverageSample) -> Self {
        Self {
            language: &sample.language,
            case: &sample.language,
            source_path: &sample.source_path,
            source_sha256: &sample.source_sha256,
            expected_parse_diagnostics: sample.expected_parse_diagnostics,
            expected_facts: &sample.expected_facts,
            negative_controls: &sample.negative_controls,
            license_evidence_files: &sample.provenance.license_evidence_files,
            source_selection: sample.provenance.source_selection.as_ref(),
        }
    }

    fn control(control: &'a CoverageControl) -> Self {
        Self {
            language: &control.language,
            case: &control.name,
            source_path: &control.source_path,
            source_sha256: &control.source_sha256,
            expected_parse_diagnostics: control.expected_parse_diagnostics,
            expected_facts: &control.expected_facts,
            negative_controls: &control.negative_controls,
            license_evidence_files: &control.license_evidence_files,
            source_selection: None,
        }
    }
}

fn run_sample(
    repo_root: &Path,
    binary: &Path,
    sample: SampleInput<'_>,
) -> Result<LanguageResult, CoverageError> {
    let source_path = repo_root.join(sample.source_path);
    let canonical_root = fs::canonicalize(repo_root)?;
    let canonical_source = fs::canonicalize(&source_path).map_err(|error| {
        CoverageError(format!(
            "missing source for {} at {}: {error}",
            sample.case,
            source_path.display()
        ))
    })?;
    if !canonical_source.starts_with(&canonical_root) || !canonical_source.is_file() {
        return Err(CoverageError(format!(
            "source for {} is outside the repository or is not a file: {}",
            sample.case,
            source_path.display()
        )));
    }
    validate_source_hash(&canonical_source, sample.source_sha256)?;
    validate_local_license_evidence(repo_root, sample.case, sample.license_evidence_files)?;
    if let Some(selection) = sample.source_selection {
        validate_source_selection(repo_root, &canonical_source, sample.case, selection)?;
    }
    let file_name = canonical_source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            CoverageError(format!(
                "corpus source filename is not UTF-8: {}",
                canonical_source.display()
            ))
        })?
        .to_string();

    let temp = TempRunDir::create()?;
    let copied_source = temp.path().join(&file_name);
    fs::copy(&canonical_source, &copied_source).map_err(|error| {
        CoverageError(format!(
            "failed to stage {} corpus input: {error}",
            sample.case
        ))
    })?;
    let database_path = temp.path().join("artifact.sqlite");
    let output = Command::new(binary)
        .current_dir(repo_root)
        .args([
            "scan",
            "--root",
            temp.path()
                .to_str()
                .ok_or_else(|| CoverageError("temporary corpus path is not UTF-8".to_string()))?,
            "--db",
            database_path
                .to_str()
                .ok_or_else(|| CoverageError("temporary artifact path is not UTF-8".to_string()))?,
            "--json",
            "--level",
            "full",
            "--jobs",
            "1",
        ])
        .output()
        .map_err(|error| {
            CoverageError(format!(
                "failed to run extractor for {}: {error}",
                sample.case
            ))
        })?;
    if !output.status.success() {
        return Err(CoverageError(format!(
            "extractor failed for {} with {}: {}",
            sample.case,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let report: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        CoverageError(format!(
            "extractor returned invalid JSON for {}: {error}",
            sample.language
        ))
    })?;
    let health = validate_scan_report(&report, sample.case)?;
    let files_scanned = health.files_scanned;
    let files_failed = health.files_failed;
    let files_unsupported = health.files_unsupported;
    let diagnostics_reported = health.parse_diagnostics;

    let connection = Connection::open(&database_path)?;
    let file_rows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM files WHERE path = ?1 AND language = ?2 AND status = 'indexed'",
        rusqlite::params![file_name, sample.language],
        |row| row.get(0),
    )?;
    let file_rows = count_to_u64(file_rows)?;
    if file_rows != 1 {
        return Err(CoverageError(format!(
            "{} did not persist exactly one indexed {} source file (found {})",
            sample.case, file_name, file_rows
        )));
    }
    let diagnostics_stored: i64 = connection.query_row(
        "SELECT COUNT(*) FROM parse_diagnostics WHERE path = ?1 AND language = ?2",
        rusqlite::params![file_name, sample.language],
        |row| row.get(0),
    )?;
    let diagnostics_stored = count_to_u64(diagnostics_stored)?;
    if diagnostics_stored != diagnostics_reported
        || diagnostics_stored != sample.expected_parse_diagnostics
    {
        return Err(CoverageError(format!(
            "{} diagnostic count mismatch: expected {}, report {}, SQLite {}",
            sample.case,
            sample.expected_parse_diagnostics,
            diagnostics_reported,
            diagnostics_stored
        )));
    }

    let mut facts = verify_fact_assertions(
        &connection,
        &file_name,
        sample.language,
        sample.expected_facts,
    )?;
    facts.extend(verify_fact_assertions(
        &connection,
        &file_name,
        sample.language,
        sample.negative_controls,
    )?);
    drop(connection);

    Ok(LanguageResult {
        language: sample.language.to_string(),
        case: sample.case.to_string(),
        file_name,
        files_scanned,
        files_failed,
        files_unsupported,
        parse_diagnostics: diagnostics_stored,
        facts,
    })
}

fn validate_scan_report(report: &Value, case: &str) -> Result<ScanReportHealth, CoverageError> {
    let status = report
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| CoverageError(format!("extractor report for {} is missing status", case)))?;
    if status != "ok" {
        return Err(CoverageError(format!(
            "extractor report for {} has status {}: {}",
            case,
            status,
            serde_json::to_string(report.get("errors").unwrap_or(&Value::Null))?
        )));
    }
    let counts = report
        .get("counts")
        .ok_or_else(|| CoverageError(format!("extractor report for {} is missing counts", case)))?;
    let files_scanned = count_value(counts, "files_scanned")?;
    let files_failed = count_value(counts, "files_failed")?;
    let files_unsupported = count_value(counts, "files_unsupported")?;
    let parse_diagnostics = counts
        .get("totals")
        .and_then(|totals| totals.get("parse_diagnostics"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            CoverageError(format!(
                "extractor report for {} is missing parse_diagnostics count",
                case
            ))
        })?;
    let errors = report
        .get("errors")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CoverageError(format!(
                "extractor report for {} is missing errors array",
                case
            ))
        })?;
    let warnings = report
        .get("warnings")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CoverageError(format!(
                "extractor report for {} is missing warnings array",
                case
            ))
        })?;
    if files_scanned != 1
        || files_failed != 0
        || files_unsupported != 0
        || !errors.is_empty()
        || !warnings.is_empty()
    {
        return Err(CoverageError(format!(
            "{} scan health failed: files_scanned={} files_failed={} files_unsupported={} errors={} warnings={}",
            case,
            files_scanned,
            files_failed,
            files_unsupported,
            errors.len(),
            warnings.len()
        )));
    }
    Ok(ScanReportHealth {
        files_scanned,
        files_failed,
        files_unsupported,
        parse_diagnostics,
    })
}

fn validate_local_license_evidence(
    repo_root: &Path,
    case: &str,
    evidence_files: &[LicenseEvidenceFile],
) -> Result<(), CoverageError> {
    let canonical_root = fs::canonicalize(repo_root)?;
    for evidence in evidence_files {
        let path = repo_root.join(&evidence.path);
        let canonical_path = fs::canonicalize(&path).map_err(|error| {
            CoverageError(format!(
                "missing offline license evidence for {} at {}: {error}",
                case,
                path.display()
            ))
        })?;
        if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
            return Err(CoverageError(format!(
                "offline license evidence for {} escapes the repository or is not a file: {}",
                case,
                path.display()
            )));
        }
        validate_source_hash(&canonical_path, &evidence.sha256)?;
    }
    Ok(())
}

fn validate_source_selection(
    repo_root: &Path,
    source_path: &Path,
    case: &str,
    selection: &SourceSelection,
) -> Result<(), CoverageError> {
    let canonical_root = fs::canonicalize(repo_root)?;
    let container_path = repo_root.join(&selection.container_path);
    let canonical_container = fs::canonicalize(&container_path).map_err(|error| {
        CoverageError(format!(
            "missing pinned source container for {} at {}: {error}",
            case,
            container_path.display()
        ))
    })?;
    if !canonical_container.starts_with(&canonical_root) || !canonical_container.is_file() {
        return Err(CoverageError(format!(
            "pinned source container for {} escapes the repository or is not a file: {}",
            case,
            container_path.display()
        )));
    }
    validate_source_hash(&canonical_container, &selection.container_sha256)?;
    let container = fs::read(&canonical_container)?;
    let source = fs::read(source_path)?;
    let start = usize::try_from(selection.start_byte).map_err(|_| {
        CoverageError(format!(
            "source selection start exceeds platform range for {}",
            case
        ))
    })?;
    let end = usize::try_from(selection.end_byte).map_err(|_| {
        CoverageError(format!(
            "source selection end exceeds platform range for {}",
            case
        ))
    })?;
    let selected = container.get(start..end).ok_or_else(|| {
        CoverageError(format!(
            "source selection exceeds its container for {}",
            case
        ))
    })?;
    if selected != source {
        return Err(CoverageError(format!(
            "vendored source bytes do not match the pinned container selection for {}",
            case
        )));
    }
    let line_start = 1 + container[..start]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count() as u64;
    let line_end = 1 + container[..end]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count() as u64;
    if line_start != selection.line_start || line_end != selection.line_end {
        return Err(CoverageError(format!(
            "source selection line range does not match the pinned container for {}",
            case
        )));
    }
    Ok(())
}

fn count_value(value: &Value, key: &str) -> Result<u64, CoverageError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CoverageError(format!("extractor report is missing count {}", key)))
}

fn count_to_u64(value: i64) -> Result<u64, CoverageError> {
    u64::try_from(value)
        .map_err(|_| CoverageError("SQLite returned a negative row count".to_string()))
}

fn required_value<'a>(
    args: &'a [String],
    index: usize,
    option: &str,
) -> Result<&'a str, CoverageUsageError> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| CoverageUsageError(format!("missing value for {}", option)))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

fn resolve_repo_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

struct TempRunDir(PathBuf);

impl TempRunDir {
    fn create() -> Result<Self, CoverageError> {
        let base = std::env::temp_dir();
        for _ in 0..100 {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("julie-coverage-{}-{}", std::process::id(), id));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(CoverageError(
            "could not allocate a unique temporary corpus directory".to_string(),
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRunDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn report_with_health_fields() -> Value {
        serde_json::json!({
            "status": "ok",
            "counts": {
                "files_scanned": 1,
                "files_failed": 0,
                "files_unsupported": 0,
                "totals": {"parse_diagnostics": 0}
            },
            "errors": [],
            "warnings": []
        })
    }

    #[test]
    fn missing_source_hash_fails_before_extraction() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("source.rs");
        fs::write(&source, "fn save() {}\n").expect("write source");

        let error = validate_source_hash(&source, &"0".repeat(64))
            .expect_err("hash mismatch must fail before invoking the CLI");

        assert!(error.to_string().contains("content hash mismatch"));
    }

    #[test]
    fn traversal_paths_are_rejected_in_manifest_samples() {
        assert!(validate_relative_path("../outside.rs").is_err());
        assert!(validate_relative_path("samples/source.rs").is_ok());
    }

    #[test]
    fn argument_parser_requires_a_value_after_language() {
        let args = vec!["--language".to_string()];

        let error = CoveragePlan::from_args(&args).expect_err("language value is required");

        assert_eq!(error.to_string(), "missing value for --language");
    }

    #[test]
    fn scan_report_requires_an_errors_array() {
        let mut report = report_with_health_fields();
        report
            .as_object_mut()
            .expect("report object")
            .remove("errors");

        let error = validate_scan_report(&report, "sample")
            .expect_err("a missing errors field is malformed health data");

        assert!(error.to_string().contains("missing errors array"));
    }

    #[test]
    fn scan_report_requires_a_warnings_array() {
        let mut report = report_with_health_fields();
        report
            .as_object_mut()
            .expect("report object")
            .remove("warnings");

        let error = validate_scan_report(&report, "sample")
            .expect_err("a missing warnings field is malformed health data");

        assert!(error.to_string().contains("missing warnings array"));
    }
}
