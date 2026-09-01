use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{
    ComplexityMetric, ExtractionResults, Identifier, Literal, ParseDiagnostic, PendingRelationship,
    Relationship, SourceRegion, StructuralFact, Symbol, TypeArgument, TypeArgumentUsage, TypeInfo,
};
use crate::pipeline::{detect_language_for_path, extract_canonical};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct CapabilityMatrix {
    languages: Vec<CapabilityRow>,
}

#[derive(Debug, Deserialize)]
struct CapabilityRow {
    language: String,
    fixtures: Vec<FixtureRow>,
}

#[derive(Debug, Deserialize)]
struct FixtureRow {
    name: String,
    source: String,
    expected: String,
    #[serde(default)]
    sources: Vec<String>,
}

impl FixtureRow {
    fn source_paths(&self) -> Vec<&str> {
        if self.sources.is_empty() {
            return vec![self.source.as_str()];
        }

        assert_eq!(
            self.sources.first().map(String::as_str),
            Some(self.source.as_str()),
            "fixture {} must list source as sources[0]",
            self.name
        );
        let mut seen = BTreeSet::new();
        for source in &self.sources {
            assert!(
                seen.insert(source),
                "fixture {} lists duplicate source {}",
                self.name,
                source
            );
        }
        self.sources.iter().map(String::as_str).collect()
    }
}

#[test]
fn qml_cross_file_fixture_declares_complete_source_list() {
    let root = workspace_root();
    let fixture = load_matrix(&root)
        .languages
        .into_iter()
        .find(|row| row.language == "qml")
        .and_then(|row| {
            row.fixtures
                .into_iter()
                .find(|fixture| fixture.name == "cross_file")
        })
        .expect("QML cross-file fixture");

    assert!(fixture.source_paths().len() >= 3);
}

fn first_difference(actual: &Value, expected: &Value, path: &str) -> String {
    if actual == expected {
        return "none".to_string();
    }
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            let keys = actual
                .keys()
                .chain(expected.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                let next = format!("{path}.{key}");
                match (actual.get(key), expected.get(key)) {
                    (Some(actual), Some(expected)) => {
                        if actual != expected {
                            return first_difference(actual, expected, &next);
                        }
                    }
                    _ => return next,
                }
            }
            path.to_string()
        }
        (Value::Array(actual), Value::Array(expected)) => {
            if actual.len() != expected.len() {
                return format!("{path}.length");
            }
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                if actual != expected {
                    return first_difference(actual, expected, &format!("{path}[{index}]"));
                }
            }
            path.to_string()
        }
        _ => format!("{path}: actual={actual} expected={expected}"),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedExtraction {
    symbols: Vec<NormalizedSymbol>,
    relationships: Vec<NormalizedRelationship>,
    pending_relationships: Vec<NormalizedPendingRelationship>,
    structured_pending_relationships: Vec<NormalizedStructuredPendingRelationship>,
    identifiers: Vec<NormalizedIdentifier>,
    types: Vec<NormalizedTypeInfo>,
    #[serde(default)]
    parse_diagnostics: Vec<NormalizedParseDiagnostic>,
    #[serde(default)]
    structural_facts: Vec<NormalizedStructuralFact>,
    #[serde(default)]
    complexity_metrics: Vec<NormalizedComplexityMetric>,
    #[serde(default)]
    literals: Vec<NormalizedLiteral>,
    #[serde(default)]
    source_regions: Vec<NormalizedSourceRegion>,
    #[serde(default)]
    type_argument_usages: Vec<NormalizedTypeArgumentUsage>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedSymbol {
    key: String,
    name: String,
    kind: String,
    language: String,
    file_path: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    body_span: Option<NormalizedBodySpan>,
    body_hash: Option<String>,
    signature: Option<String>,
    doc_comment: Option<String>,
    visibility: Option<String>,
    parent_key: Option<String>,
    metadata: Option<Value>,
    annotations: Value,
    semantic_group: Option<String>,
    confidence: Option<String>,
    content_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedBodySpan {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedRelationship {
    from_key: String,
    to_key: String,
    kind: String,
    file_path: String,
    line_number: u32,
    span: Option<NormalizedBodySpan>,
    reference_site_is_exact: bool,
    confidence: String,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedPendingRelationship {
    from_key: String,
    callee_name: String,
    kind: String,
    file_path: String,
    line_number: u32,
    confidence: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedStructuredPendingRelationship {
    pending: NormalizedPendingRelationship,
    target: NormalizedUnresolvedTarget,
    caller_scope_key: Option<String>,
    span: Option<NormalizedBodySpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receiver_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedUnresolvedTarget {
    display_name: String,
    terminal_name: String,
    receiver: Option<String>,
    namespace_path: Vec<String>,
    import_context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedIdentifier {
    key: String,
    name: String,
    kind: String,
    language: String,
    file_path: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    containing_key: Option<String>,
    target_key: Option<String>,
    confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receiver_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedTypeInfo {
    symbol_key: String,
    resolved_type: String,
    generic_params: Option<Vec<String>>,
    constraints: Option<Vec<String>>,
    is_inferred: bool,
    language: String,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedParseDiagnostic {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedStructuralFact {
    pattern_id: String,
    capture_name: String,
    node_kind: String,
    language: String,
    file_path: String,
    containing_key: Option<String>,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    confidence: String,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedComplexityMetric {
    scope: String,
    symbol_key: Option<String>,
    algorithm_id: String,
    language: String,
    file_path: String,
    covered_lines: u32,
    covered_bytes: u32,
    decision_count: u32,
    loop_count: u32,
    max_nesting_depth: u32,
    parameter_count: Option<u32>,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedLiteral {
    literal_text: String,
    kind: String,
    carrier: Option<String>,
    arg_position: u32,
    language: String,
    file_path: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    containing_key: Option<String>,
    confidence: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedSourceRegion {
    kind: String,
    language: String,
    file_path: String,
    containing_key: Option<String>,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    start_byte: u32,
    end_byte: u32,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedTypeArgumentUsage {
    identifier_key: String,
    language: String,
    file_path: String,
    arguments: Vec<NormalizedTypeArgument>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct NormalizedTypeArgument {
    ordinal: u32,
    type_name: String,
    children: Vec<NormalizedTypeArgument>,
}

#[test]
fn no_language_emits_per_identifier_code_context() {
    let root = workspace_root();
    let matrix = load_matrix(&root);
    let mut languages_with_identifiers = BTreeSet::new();

    for row in matrix.languages {
        for fixture in row.fixtures {
            let results = extract_fixture(&root, &row.language, &fixture);

            if !results.identifiers.is_empty() {
                languages_with_identifiers.insert(row.language.clone());
            }
            for identifier in &results.identifiers {
                assert_eq!(
                    identifier.code_context, None,
                    "{}:{} identifier `{}` still carries code_context",
                    row.language, fixture.name, identifier.name
                );
            }
        }
    }

    assert!(
        languages_with_identifiers.len() > 20,
        "expected identifier coverage across the fixture corpus, saw {languages_with_identifiers:?}"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct TestRoleFlags {
    is_test: bool,
    lifecycle: bool,
    container: bool,
}

impl TestRoleFlags {
    fn any(&self) -> bool {
        self.is_test || self.lifecycle || self.container
    }
}

fn flags_written_for_role(role: &str) -> Option<TestRoleFlags> {
    match role {
        "test_case" | "parameterized_test" => Some(TestRoleFlags {
            is_test: true,
            lifecycle: false,
            container: false,
        }),
        "fixture_setup" | "fixture_teardown" => Some(TestRoleFlags {
            is_test: true,
            lifecycle: true,
            container: false,
        }),
        "test_container" => Some(TestRoleFlags {
            is_test: false,
            lifecycle: false,
            container: true,
        }),
        _ => None,
    }
}

fn flags_in_metadata(metadata: Option<&Value>) -> TestRoleFlags {
    let flag = |key: &str| {
        metadata
            .and_then(|metadata| metadata.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    TestRoleFlags {
        is_test: flag("is_test"),
        lifecycle: flag("test_lifecycle"),
        container: flag("test_container"),
    }
}

#[test]
fn every_golden_test_boolean_carries_an_agreeing_test_role() {
    let root = workspace_root();
    let matrix = load_matrix(&root);
    let mut violations = Vec::new();
    let mut languages = BTreeSet::new();
    let mut flagged_symbols = 0usize;

    for row in matrix.languages {
        for fixture in row.fixtures {
            let expected_path = root.join(&fixture.expected);
            let expected_json = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
                panic!(
                    "failed to read expected golden output for {}:{} at {}: {}",
                    row.language,
                    fixture.name,
                    expected_path.display(),
                    err
                )
            });
            let expected: Value = serde_json::from_str(&expected_json).unwrap_or_else(|err| {
                panic!(
                    "failed to parse expected golden output for {}:{} at {}: {}",
                    row.language,
                    fixture.name,
                    expected_path.display(),
                    err
                )
            });
            let symbols = expected
                .get("symbols")
                .and_then(Value::as_array)
                .unwrap_or_else(|| {
                    panic!(
                        "expected golden output for {}:{} has no symbols array",
                        row.language, fixture.name
                    )
                });

            for symbol in symbols {
                let metadata = symbol.get("metadata");
                let flags = flags_in_metadata(metadata);
                let role = metadata
                    .and_then(|metadata| metadata.get("test_role"))
                    .and_then(Value::as_str);
                if !flags.any() && role.is_none() {
                    continue;
                }

                flagged_symbols += 1;
                languages.insert(row.language.clone());
                let name = symbol.get("name").and_then(Value::as_str).unwrap_or("?");
                let where_ = format!("{}:{} `{name}`", row.language, fixture.name);

                let Some(role) = role else {
                    violations.push(format!("{where_} carries {flags:?} without a test_role"));
                    continue;
                };
                let Some(written) = flags_written_for_role(role) else {
                    violations.push(format!("{where_} carries unknown test_role `{role}`"));
                    continue;
                };
                if written != flags {
                    violations.push(format!(
                        "{where_} has test_role `{role}` but {flags:?} instead of {written:?}"
                    ));
                }
            }
        }
    }

    assert!(
        languages.len() >= 30,
        "scan should cover the whole registered corpus, saw only {languages:?}"
    );
    assert!(
        flagged_symbols >= 400,
        "scan should see the corpus test symbols, saw only {flagged_symbols}"
    );
    assert!(
        violations.is_empty(),
        "{} golden symbols disagree with their test_role:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn golden_fixtures_match_canonical_extraction() {
    let root = workspace_root();
    let matrix = load_matrix(&root);
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let selected_language = std::env::var("JULIE_GOLDEN_LANGUAGE").ok();
    let mut seen = BTreeSet::new();
    let mut matched_language = false;

    for row in matrix.languages {
        if selected_language
            .as_deref()
            .is_some_and(|selected| selected != row.language)
        {
            continue;
        }
        matched_language = true;
        assert!(
            !row.fixtures.is_empty(),
            "language {} has no golden fixtures",
            row.language
        );

        for fixture in row.fixtures {
            let case_key = format!("{}:{}", row.language, fixture.name);
            assert!(
                seen.insert(case_key.clone()),
                "duplicate fixture {case_key}"
            );

            let expected_path = root.join(&fixture.expected);
            let normalized = normalize(extract_fixture(&root, &row.language, &fixture));
            let actual_json = serde_json::to_string_pretty(&normalized).unwrap();

            if update {
                if let Some(parent) = expected_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&expected_path, format!("{actual_json}\n")).unwrap();
                continue;
            }

            let expected_json = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
                panic!(
                    "missing expected golden output for {} at {}: {}. Run UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors golden",
                    case_key,
                    expected_path.display(),
                    err
                )
            });
            let expected: NormalizedExtraction = serde_json::from_str(&expected_json)
                .unwrap_or_else(|err| {
                    panic!(
                        "invalid expected golden output for {} at {}: {}",
                        case_key,
                        expected_path.display(),
                        err
                    )
                });

            if expected != normalized {
                let actual_value = serde_json::to_value(&normalized).unwrap();
                let expected_value = serde_json::to_value(&expected).unwrap();
                panic!(
                    "golden mismatch for {case_key} at {}: {}\nactual: {}",
                    expected_path.display(),
                    first_difference(&actual_value, &expected_value, "$"),
                    actual_json
                );
            }
        }
    }

    if let Some(selected) = selected_language {
        assert!(
            matched_language,
            "JULIE_GOLDEN_LANGUAGE={selected} did not match a capability-matrix language"
        );
    }
}

fn extract_fixture(root: &Path, language: &str, fixture: &FixtureRow) -> ExtractionResults {
    let mut merged = ExtractionResults::empty();
    let mut type_keys = BTreeSet::new();

    for source_path in fixture.source_paths() {
        let path = root.join(source_path);
        let source =
            normalize_fixture_line_endings(fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!(
                    "failed to read source for {}:{} at {}: {}",
                    language,
                    fixture.name,
                    path.display(),
                    err
                )
            }));
        let detected = detect_language_for_path(source_path)
            .unwrap_or_else(|err| panic!("failed to detect language for {source_path}: {err}"));
        assert_eq!(
            detected, language,
            "fixture {}:{} source {} must route through its registry language",
            language, fixture.name, source_path
        );
        let results = extract_canonical(source_path, &source, root).unwrap_or_else(|err| {
            panic!(
                "extract_canonical failed for {}:{} source {}: {err}",
                language, fixture.name, source_path
            )
        });
        for key in results.types.keys() {
            assert!(
                type_keys.insert(key.clone()),
                "fixture {}:{} produced duplicate type-map key {}",
                language,
                fixture.name,
                key
            );
        }
        merged.extend(results);
    }

    merged
}

fn normalize_fixture_line_endings(source: String) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("julie-extractors crate should live under crates/")
        .to_path_buf()
}

fn load_matrix(root: &Path) -> CapabilityMatrix {
    let matrix_path = root.join("fixtures/extraction/capabilities.json");
    let json = fs::read_to_string(&matrix_path).unwrap_or_else(|err| {
        panic!(
            "failed to read capability matrix at {}: {}",
            matrix_path.display(),
            err
        )
    });
    serde_json::from_str(&json).unwrap_or_else(|err| {
        panic!(
            "failed to parse capability matrix at {}: {}",
            matrix_path.display(),
            err
        )
    })
}

fn normalize(results: ExtractionResults) -> NormalizedExtraction {
    let symbol_keys = symbol_key_map(&results.symbols);
    let mut symbols: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| normalize_symbol(symbol, &symbol_keys))
        .collect();
    let mut relationships: Vec<_> = results
        .relationships
        .iter()
        .map(|relationship| normalize_relationship(relationship, &symbol_keys))
        .collect();
    let mut pending_relationships: Vec<_> = results
        .pending_relationships
        .iter()
        .map(|pending| normalize_pending(pending, &symbol_keys))
        .collect();
    let mut structured_pending_relationships: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .map(|pending| normalize_structured_pending(pending, &symbol_keys))
        .collect();
    let mut identifiers: Vec<_> = results
        .identifiers
        .iter()
        .map(|identifier| normalize_identifier(identifier, &symbol_keys))
        .collect();
    let mut types: Vec<_> = results
        .types
        .values()
        .map(|type_info| normalize_type(type_info, &symbol_keys))
        .collect();
    let mut parse_diagnostics: Vec<_> = results
        .parse_diagnostics
        .iter()
        .map(normalize_parse_diagnostic)
        .collect();
    let mut structural_facts: Vec<_> = results
        .structural_facts
        .iter()
        .map(|fact| normalize_structural_fact(fact, &symbol_keys))
        .collect();
    let mut complexity_metrics: Vec<_> = results
        .complexity_metrics
        .iter()
        .map(|metric| normalize_complexity_metric(metric, &symbol_keys))
        .collect();
    let mut literals: Vec<_> = results
        .literals
        .iter()
        .map(|literal| normalize_literal(literal, &symbol_keys))
        .collect();
    let mut source_regions: Vec<_> = results
        .source_regions
        .iter()
        .map(|region| normalize_source_region(region, &symbol_keys))
        .collect();
    let identifier_keys = identifier_key_map(&results.identifiers);
    let mut type_argument_usages: Vec<_> = results
        .type_argument_usages
        .iter()
        .map(|usage| normalize_type_argument_usage(usage, &identifier_keys))
        .collect();

    sort_json(&mut symbols);
    sort_json(&mut relationships);
    sort_json(&mut pending_relationships);
    sort_json(&mut structured_pending_relationships);
    sort_json(&mut identifiers);
    sort_json(&mut types);
    sort_json(&mut parse_diagnostics);
    sort_json(&mut structural_facts);
    sort_json(&mut complexity_metrics);
    sort_json(&mut literals);
    sort_json(&mut source_regions);
    sort_json(&mut type_argument_usages);

    NormalizedExtraction {
        symbols,
        relationships,
        pending_relationships,
        structured_pending_relationships,
        identifiers,
        types,
        parse_diagnostics,
        structural_facts,
        complexity_metrics,
        literals,
        source_regions,
        type_argument_usages,
    }
}

fn symbol_key_map(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol_key(symbol)))
        .collect()
}

fn symbol_key(symbol: &Symbol) -> String {
    format!(
        "{}:{}:{}:{}",
        symbol.file_path, symbol.name, symbol.start_line, symbol.start_column
    )
}

fn normalize_symbol(symbol: &Symbol, symbol_keys: &HashMap<String, String>) -> NormalizedSymbol {
    NormalizedSymbol {
        key: symbol_key(symbol),
        name: symbol.name.clone(),
        kind: symbol.kind.to_string(),
        language: symbol.language.clone(),
        file_path: symbol.file_path.clone(),
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
        body_span: symbol.body_span.map(normalize_body_span),
        body_hash: symbol.body_hash.clone(),
        signature: symbol.signature.clone(),
        doc_comment: symbol.doc_comment.clone(),
        visibility: symbol.visibility.as_ref().map(ToString::to_string),
        parent_key: symbol
            .parent_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        metadata: symbol.metadata.as_ref().map(sorted_json_map),
        annotations: serde_json::to_value(&symbol.annotations).unwrap(),
        semantic_group: symbol.semantic_group.clone(),
        confidence: symbol.confidence.map(normalize_confidence),
        content_type: symbol.content_type.clone(),
    }
}

fn normalize_body_span(span: crate::base::BodySpan) -> NormalizedBodySpan {
    NormalizedBodySpan {
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
    }
}

fn normalize_relationship(
    relationship: &Relationship,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedRelationship {
    NormalizedRelationship {
        from_key: lookup_symbol_key(&relationship.from_symbol_id, symbol_keys),
        to_key: lookup_symbol_key(&relationship.to_symbol_id, symbol_keys),
        kind: relationship.kind.to_string(),
        file_path: relationship.file_path.clone(),
        line_number: relationship.line_number,
        span: relationship.span.map(normalize_body_span),
        reference_site_is_exact: relationship.reference_site_is_exact,
        confidence: normalize_confidence(relationship.confidence),
        metadata: relationship.metadata.as_ref().map(sorted_json_map),
    }
}

fn normalize_pending(
    pending: &PendingRelationship,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedPendingRelationship {
    NormalizedPendingRelationship {
        from_key: lookup_symbol_key(&pending.from_symbol_id, symbol_keys),
        callee_name: pending.callee_name.clone(),
        kind: pending.kind.to_string(),
        file_path: pending.file_path.clone(),
        line_number: pending.line_number,
        confidence: normalize_confidence(pending.confidence),
    }
}

fn normalize_structured_pending(
    pending: &StructuredPendingRelationship,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedStructuredPendingRelationship {
    NormalizedStructuredPendingRelationship {
        pending: normalize_pending(&pending.pending, symbol_keys),
        target: normalize_target(&pending.target),
        caller_scope_key: pending
            .caller_scope_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        span: pending.span.map(normalize_body_span),
        receiver_type: pending.receiver_type.clone(),
    }
}

fn normalize_target(target: &UnresolvedTarget) -> NormalizedUnresolvedTarget {
    NormalizedUnresolvedTarget {
        display_name: target.display_name.clone(),
        terminal_name: target.terminal_name.clone(),
        receiver: target.receiver.clone(),
        namespace_path: target.namespace_path.clone(),
        import_context: target.import_context.clone(),
    }
}

fn normalize_identifier(
    identifier: &Identifier,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedIdentifier {
    NormalizedIdentifier {
        key: identifier_key(identifier),
        name: identifier.name.clone(),
        kind: identifier.kind.to_string(),
        language: identifier.language.clone(),
        file_path: identifier.file_path.clone(),
        start_line: identifier.start_line,
        start_column: identifier.start_column,
        end_line: identifier.end_line,
        end_column: identifier.end_column,
        start_byte: identifier.start_byte,
        end_byte: identifier.end_byte,
        containing_key: identifier
            .containing_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        target_key: identifier
            .target_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        confidence: normalize_confidence(identifier.confidence),
        receiver_type: identifier.receiver_type.clone(),
    }
}

fn normalize_type(
    type_info: &TypeInfo,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedTypeInfo {
    NormalizedTypeInfo {
        symbol_key: lookup_symbol_key(&type_info.symbol_id, symbol_keys),
        resolved_type: type_info.resolved_type.clone(),
        generic_params: type_info.generic_params.clone(),
        constraints: type_info.constraints.clone(),
        is_inferred: type_info.is_inferred,
        language: type_info.language.clone(),
        metadata: type_info.metadata.as_ref().map(sorted_json_map),
    }
}

fn normalize_parse_diagnostic(diagnostic: &ParseDiagnostic) -> NormalizedParseDiagnostic {
    NormalizedParseDiagnostic {
        kind: format!("{:?}", diagnostic.kind),
        message: diagnostic.message.clone(),
        start_line: diagnostic.start_line,
        start_column: diagnostic.start_column,
        end_line: diagnostic.end_line,
        end_column: diagnostic.end_column,
        start_byte: diagnostic.start_byte,
        end_byte: diagnostic.end_byte,
    }
}

fn identifier_key(identifier: &Identifier) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        identifier.file_path,
        identifier.name,
        identifier.kind,
        identifier.start_line,
        identifier.start_column
    )
}

fn identifier_key_map(identifiers: &[Identifier]) -> HashMap<String, String> {
    identifiers
        .iter()
        .map(|identifier| (identifier.id.clone(), identifier_key(identifier)))
        .collect()
}

fn normalize_structural_fact(
    fact: &StructuralFact,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedStructuralFact {
    NormalizedStructuralFact {
        pattern_id: fact.pattern_id.clone(),
        capture_name: fact.capture_name.clone(),
        node_kind: fact.node_kind.clone(),
        language: fact.language.clone(),
        file_path: fact.file_path.clone(),
        containing_key: fact
            .containing_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        start_line: fact.start_line,
        start_column: fact.start_column,
        end_line: fact.end_line,
        end_column: fact.end_column,
        start_byte: fact.start_byte,
        end_byte: fact.end_byte,
        confidence: normalize_confidence(fact.confidence),
        metadata: fact.metadata.as_ref().map(sorted_json_map),
    }
}

fn normalize_complexity_metric(
    metric: &ComplexityMetric,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedComplexityMetric {
    NormalizedComplexityMetric {
        scope: metric.scope.clone(),
        symbol_key: metric
            .symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        algorithm_id: metric.algorithm_id.clone(),
        language: metric.language.clone(),
        file_path: metric.file_path.clone(),
        covered_lines: metric.covered_lines,
        covered_bytes: metric.covered_bytes,
        decision_count: metric.decision_count,
        loop_count: metric.loop_count,
        max_nesting_depth: metric.max_nesting_depth,
        parameter_count: metric.parameter_count,
        start_line: metric.start_line,
        start_column: metric.start_column,
        end_line: metric.end_line,
        end_column: metric.end_column,
        start_byte: metric.start_byte,
        end_byte: metric.end_byte,
        metadata: metric.metadata.as_ref().map(sorted_json_map),
    }
}

fn normalize_literal(
    literal: &Literal,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedLiteral {
    NormalizedLiteral {
        literal_text: literal.literal_text.clone(),
        kind: literal.kind.as_str().to_string(),
        carrier: literal.carrier.clone(),
        arg_position: literal.arg_position,
        language: literal.language.clone(),
        file_path: literal.file_path.clone(),
        start_line: literal.start_line,
        start_column: literal.start_column,
        end_line: literal.end_line,
        end_column: literal.end_column,
        start_byte: literal.start_byte,
        end_byte: literal.end_byte,
        containing_key: literal
            .containing_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        confidence: normalize_confidence(literal.confidence),
    }
}

fn normalize_source_region(
    region: &SourceRegion,
    symbol_keys: &HashMap<String, String>,
) -> NormalizedSourceRegion {
    NormalizedSourceRegion {
        kind: region.kind.as_str().to_string(),
        language: region.language.clone(),
        file_path: region.file_path.clone(),
        containing_key: region
            .containing_symbol_id
            .as_ref()
            .map(|id| lookup_symbol_key(id, symbol_keys)),
        start_line: region.start_line,
        start_column: region.start_column,
        end_line: region.end_line,
        end_column: region.end_column,
        start_byte: region.start_byte,
        end_byte: region.end_byte,
        metadata: region.metadata.as_ref().map(sorted_json_map),
    }
}

fn normalize_type_argument_usage(
    usage: &TypeArgumentUsage,
    identifier_keys: &HashMap<String, String>,
) -> NormalizedTypeArgumentUsage {
    NormalizedTypeArgumentUsage {
        identifier_key: identifier_keys
            .get(&usage.identifier_id)
            .cloned()
            .unwrap_or_else(|| format!("unresolved:{}", usage.identifier_id)),
        language: usage.language.clone(),
        file_path: usage.file_path.clone(),
        arguments: usage
            .arguments
            .iter()
            .map(normalize_type_argument)
            .collect(),
    }
}

fn normalize_type_argument(argument: &TypeArgument) -> NormalizedTypeArgument {
    NormalizedTypeArgument {
        ordinal: argument.ordinal,
        type_name: argument.type_name.clone(),
        children: argument
            .children
            .iter()
            .map(normalize_type_argument)
            .collect(),
    }
}

fn lookup_symbol_key(id: &str, symbol_keys: &HashMap<String, String>) -> String {
    symbol_keys
        .get(id)
        .cloned()
        .unwrap_or_else(|| format!("unresolved:{id}"))
}

fn sorted_json_map(map: &HashMap<String, Value>) -> Value {
    let sorted: BTreeMap<_, _> = map
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    serde_json::to_value(sorted).unwrap()
}

fn normalize_confidence(confidence: f32) -> String {
    format!("{confidence:.3}")
}

fn sort_json<T: Serialize>(items: &mut [T]) {
    items.sort_by_key(|item| serde_json::to_string(item).unwrap());
}
