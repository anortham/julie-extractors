//! Symbols-level extraction gates the reference and text/facts families
//! uniformly across languages. `strip_to_symbols_level` is the single
//! authority on the gated set; these tests pin both directions — what a
//! symbols-level extraction may never carry, and what it must keep identical
//! to a full-level extraction of the same source.

use std::path::Path;

use crate::ExtractionLevel;
use crate::pipeline::{extract_canonical, extract_canonical_at};

fn extract_at(level: ExtractionLevel, file_path: &str, source: &str) -> crate::ExtractionResults {
    extract_canonical_at(file_path, source, Path::new("/repo"), level)
        .expect("canonical extraction should succeed")
}

struct LevelFixture {
    language: &'static str,
    file_path: &'static str,
    source: &'static str,
}

/// Rust exercises the macro registry path; sql, markdown, and regex record
/// literals OUTSIDE their identifier walks, so they prove the strip keeps the
/// level uniform instead of leaving a silent three-language literal subset.
const FIXTURES: &[LevelFixture] = &[
    LevelFixture {
        language: "rust",
        file_path: "src/lib.rs",
        source: "// comment\n/// docs\n#[derive(Debug)]\npub struct Alpha;\npub fn alpha() { helper(\"hello\"); }\npub fn helper(v: &str) -> &str { v }\n",
    },
    LevelFixture {
        language: "sql",
        file_path: "db/schema.sql",
        source: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT DEFAULT 'anon');\nINSERT INTO users (name) VALUES ('alpha');\n",
    },
    LevelFixture {
        language: "markdown",
        file_path: "docs/readme.md",
        source: "# Title\n\nSome prose with a [link](https://example.com).\n\n```rust\nfn embedded() {}\n```\n",
    },
    LevelFixture {
        language: "regex",
        file_path: "patterns/route.regex",
        source: "^/api/(?<version>v[0-9]+)/users/(?<id>[0-9]+)$",
    },
];

#[test]
fn symbols_level_never_carries_gated_families_for_any_fixture_language() {
    for fixture in FIXTURES {
        let results = extract_at(ExtractionLevel::Symbols, fixture.file_path, fixture.source);
        assert!(
            results.identifiers.is_empty(),
            "{}: identifiers must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.literals.is_empty(),
            "{}: literals must be empty at symbols level (even for languages that record them outside the identifier walk)",
            fixture.language
        );
        assert!(
            results.type_argument_usages.is_empty(),
            "{}: type argument usages must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.source_regions.is_empty(),
            "{}: source regions must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.structural_facts.is_empty(),
            "{}: structural facts must be empty at symbols level",
            fixture.language
        );
    }
}

#[test]
fn symbols_level_keeps_symbols_and_relationships_identical_to_full() {
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        let symbols = extract_at(ExtractionLevel::Symbols, fixture.file_path, fixture.source);
        assert_eq!(
            full.symbols.len(),
            symbols.symbols.len(),
            "{}: symbol extraction must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.relationships.len(),
            symbols.relationships.len(),
            "{}: relationships must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.pending_relationships.len(),
            symbols.pending_relationships.len(),
            "{}: pending relationships must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.parse_diagnostics.len(),
            symbols.parse_diagnostics.len(),
            "{}: parse diagnostics must be identical across levels",
            fixture.language
        );
    }
}

#[test]
fn full_level_fixtures_actually_exercise_the_gated_families() {
    let mut saw_identifiers = false;
    let mut saw_literals = false;
    let mut saw_regions = false;
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        saw_identifiers |= !full.identifiers.is_empty();
        saw_literals |= !full.literals.is_empty();
        saw_regions |= !full.source_regions.is_empty();
    }
    assert!(
        saw_identifiers && saw_literals && saw_regions,
        "the fixtures must produce identifiers/literals/regions at full level, \
         or the symbols-level emptiness assertions are vacuous \
         (identifiers={saw_identifiers}, literals={saw_literals}, regions={saw_regions})"
    );
}

#[test]
fn extract_canonical_defaults_to_full_level() {
    let fixture = &FIXTURES[0];
    let default = extract_canonical(fixture.file_path, fixture.source, Path::new("/repo"))
        .expect("canonical extraction should succeed");
    let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
    assert_eq!(default.identifiers.len(), full.identifiers.len());
    assert_eq!(default.source_regions.len(), full.source_regions.len());
    assert!(
        !default.identifiers.is_empty(),
        "the default path must keep extracting the reference layer"
    );
}

#[test]
fn strip_to_level_is_the_single_authority_on_the_gated_set() {
    let fixture = &FIXTURES[0];
    let mut results = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
    let kept_symbols = results.symbols.len();
    let kept_relationships = results.relationships.len();
    results.strip_to_level(ExtractionLevel::Symbols);
    assert!(results.identifiers.is_empty());
    assert!(results.literals.is_empty());
    assert!(results.type_argument_usages.is_empty());
    assert!(results.source_regions.is_empty());
    assert!(results.structural_facts.is_empty());
    assert_eq!(results.symbols.len(), kept_symbols);
    assert_eq!(results.relationships.len(), kept_relationships);
    assert!(results.complexity_metrics.is_empty());
}

#[test]
fn facts_level_keeps_structural_facts_literals_and_type_and_member_identifiers_for_every_fixture_language()
 {
    use crate::base::IdentifierKind;
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        let facts = extract_at(ExtractionLevel::Facts, fixture.file_path, fixture.source);
        assert_eq!(
            full.structural_facts.len(),
            facts.structural_facts.len(),
            "{}: structural facts must be identical between facts and full",
            fixture.language
        );
        assert_eq!(
            full.symbols.len(),
            facts.symbols.len(),
            "{}",
            fixture.language
        );
        assert_eq!(
            full.pending_relationships.len(),
            facts.pending_relationships.len(),
            "{}",
            fixture.language
        );
        assert_eq!(
            full.literals.len(),
            facts.literals.len(),
            "{}",
            fixture.language
        );
        let kept_from_full = full
            .identifiers
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    IdentifierKind::TypeUsage | IdentifierKind::MemberAccess
                )
            })
            .count();
        assert_eq!(
            facts.identifiers.len(),
            kept_from_full,
            "{}: facts must keep exactly the type_usage and member_access identifiers",
            fixture.language
        );
        assert!(
            facts.identifiers.iter().all(|i| matches!(
                i.kind,
                IdentifierKind::TypeUsage | IdentifierKind::MemberAccess
            )),
            "{}",
            fixture.language
        );
        assert!(
            facts.type_argument_usages.is_empty(),
            "{}",
            fixture.language
        );
        assert!(facts.source_regions.is_empty(), "{}", fixture.language);
    }
}

#[test]
fn facts_level_fixtures_actually_carry_type_or_member_identifiers() {
    let any = FIXTURES.iter().any(|fixture| {
        !extract_at(ExtractionLevel::Facts, fixture.file_path, fixture.source)
            .identifiers
            .is_empty()
    });
    assert!(
        any,
        "at least one fixture must exercise the kept identifier kinds"
    );
}

#[test]
fn facts_level_round_trips_through_its_metadata_value() {
    assert_eq!(ExtractionLevel::Facts.metadata_value(), "facts");
    assert_eq!(
        ExtractionLevel::from_metadata_value("facts"),
        Some(ExtractionLevel::Facts)
    );
}

#[test]
fn references_level_keeps_every_identifier_of_full_and_drops_regions_and_type_arguments() {
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        let references = extract_at(
            ExtractionLevel::References,
            fixture.file_path,
            fixture.source,
        );
        assert_eq!(
            full.identifiers, references.identifiers,
            "{}",
            fixture.language
        );
        assert_eq!(
            full.symbols.len(),
            references.symbols.len(),
            "{}",
            fixture.language
        );
        assert_eq!(
            full.pending_relationships.len(),
            references.pending_relationships.len(),
            "{}",
            fixture.language
        );
        assert_eq!(
            full.structural_facts.len(),
            references.structural_facts.len(),
            "{}",
            fixture.language
        );
        assert_eq!(
            full.literals.len(),
            references.literals.len(),
            "{}",
            fixture.language
        );
        assert!(
            references.type_argument_usages.is_empty(),
            "{}",
            fixture.language
        );
        assert!(references.source_regions.is_empty(), "{}", fixture.language);
    }
}

#[test]
fn references_level_keeps_call_argument_counts() {
    let references = extract_at(
        ExtractionLevel::References,
        FIXTURES[0].file_path,
        FIXTURES[0].source,
    );
    let helper_call = references
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "helper")
        .expect("the rust fixture calls helper");
    assert_eq!(
        helper_call
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("arg_count")),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn references_level_round_trips_through_its_metadata_value() {
    assert_eq!(ExtractionLevel::References.metadata_value(), "references");
    assert_eq!(
        ExtractionLevel::from_metadata_value("references"),
        Some(ExtractionLevel::References)
    );
}

fn annotation_count(results: &crate::ExtractionResults) -> usize {
    results
        .symbols
        .iter()
        .map(|symbol| symbol.annotations.len())
        .sum()
}

#[test]
fn only_full_level_carries_complexity_metrics() {
    for fixture in FIXTURES {
        for level in [
            ExtractionLevel::Symbols,
            ExtractionLevel::Facts,
            ExtractionLevel::References,
        ] {
            let results = extract_at(level, fixture.file_path, fixture.source);
            assert!(
                results.complexity_metrics.is_empty(),
                "{}: complexity metrics must be empty below full level",
                fixture.language
            );
        }
    }
}

#[test]
fn symbol_annotations_are_preserved_at_references_and_full_levels() {
    let fixture = &FIXTURES[0];
    for (level, expected) in [
        (ExtractionLevel::Symbols, 0),
        (ExtractionLevel::Facts, 0),
        (ExtractionLevel::References, 1),
        (ExtractionLevel::Full, 1),
    ] {
        let results = extract_at(level, fixture.file_path, fixture.source);
        assert_eq!(annotation_count(&results), expected, "{level:?}");
    }
}

#[cfg(feature = "test-capability-matrix")]
#[test]
fn capability_matrix_preserves_annotations_at_every_extraction_level() {
    use std::collections::BTreeSet;
    use std::fs;

    use serde_json::Value;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/extraction/capabilities.json"
    ))
    .expect("capability manifest should parse");
    let rows = manifest["languages"]
        .as_array()
        .expect("capability manifest should list languages");
    let annotation_languages = rows
        .iter()
        .filter(|row| {
            row["kind_coverage"]["annotations"]["supported"]
                .as_array()
                .is_some_and(|units| !units.is_empty())
        })
        .map(|row| {
            row["language"]
                .as_str()
                .expect("annotation claimant should have a language")
        })
        .collect::<Vec<_>>();
    let annotation_owners = |expected: &Value,
                             source_path: &str,
                             source_count: usize|
     -> BTreeSet<(String, String, u32, String)> {
        let mut owners = BTreeSet::new();
        for symbol in expected["symbols"]
            .as_array()
            .expect("expected fixture should list symbols")
        {
            match symbol["file_path"].as_str() {
                Some(path) if path != source_path => continue,
                Some(_) => {}
                None if source_count == 1 => {}
                None => panic!("multi-source golden symbols must identify their source"),
            }
            for annotation in symbol["annotations"]
                .as_array()
                .expect("expected symbol should list annotations")
            {
                owners.insert((
                    symbol["name"]
                        .as_str()
                        .expect("annotated golden symbol should have a name")
                        .to_owned(),
                    symbol["kind"]
                        .as_str()
                        .expect("annotated golden symbol should have a kind")
                        .to_owned(),
                    symbol["start_byte"]
                        .as_u64()
                        .expect("annotated golden symbol should have a start byte")
                        as u32,
                    annotation["annotation_key"]
                        .as_str()
                        .expect("golden annotation should have a key")
                        .to_owned(),
                ));
            }
        }
        owners
    };

    for language in annotation_languages {
        let row = rows
            .iter()
            .find(|row| row["language"].as_str() == Some(language))
            .unwrap_or_else(|| panic!("{language} is not registered in the capability manifest"));
        let fixtures = row["fixtures"]
            .as_array()
            .unwrap_or_else(|| panic!("{language} has no registered fixtures"));
        let mut selected = None;
        for fixture in fixtures {
            let expected_path = fixture["expected"]
                .as_str()
                .unwrap_or_else(|| panic!("{language} fixture has no expected path"));
            let expected_json = fs::read_to_string(root.join(expected_path))
                .unwrap_or_else(|error| panic!("failed to read {expected_path}: {error}"));
            let expected: Value = serde_json::from_str(&expected_json)
                .unwrap_or_else(|error| panic!("failed to parse {expected_path}: {error}"));
            let source_paths = fixture["sources"]
                .as_array()
                .map(|sources| {
                    sources
                        .iter()
                        .map(|source| {
                            source
                                .as_str()
                                .expect("registered fixture source path should be a string")
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| {
                    vec![
                        fixture["source"]
                            .as_str()
                            .expect("registered fixture should have a source path"),
                    ]
                });
            for source_path in &source_paths {
                let owners = annotation_owners(&expected, source_path, source_paths.len());
                if !owners.is_empty() {
                    let source = fs::read_to_string(root.join(source_path))
                        .unwrap_or_else(|error| panic!("failed to read {source_path}: {error}"));
                    let full = extract_at(ExtractionLevel::Full, source_path, &source);
                    if full.parse_diagnostics.is_empty() {
                        selected = Some((expected_path, *source_path, owners, source));
                        break;
                    }
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (expected_path, source_path, expected_owners, source) = selected.unwrap_or_else(|| {
            panic!("{language} has no registered diagnostic-free positive annotation golden")
        });

        for level in [
            ExtractionLevel::Symbols,
            ExtractionLevel::Facts,
            ExtractionLevel::References,
            ExtractionLevel::Full,
        ] {
            let results = extract_at(level, source_path, &source);
            assert!(
                results.parse_diagnostics.is_empty(),
                "{language} {source_path} at {level:?} must have no parse diagnostics"
            );
            let actual_owners = results
                .symbols
                .iter()
                .flat_map(|symbol| {
                    symbol.annotations.iter().map(move |annotation| {
                        (
                            symbol.name.clone(),
                            serde_json::to_value(&symbol.kind)
                                .expect("symbol kind should serialize")
                                .as_str()
                                .expect("symbol kind should serialize as text")
                                .to_owned(),
                            symbol.start_byte,
                            annotation.annotation_key.clone(),
                        )
                    })
                })
                .collect::<BTreeSet<_>>();

            if matches!(level, ExtractionLevel::References | ExtractionLevel::Full) {
                assert_eq!(
                    actual_owners, expected_owners,
                    "{language} annotation owners must match {expected_path} at {level:?}"
                );
            } else {
                assert!(
                    actual_owners.is_empty(),
                    "{language} annotations must be absent at {level:?}"
                );
            }
        }
    }
}

#[test]
fn full_level_fixtures_actually_carry_complexity_metrics_and_symbol_annotations() {
    let mut saw_complexity = false;
    let mut saw_annotations = false;
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        saw_complexity |= !full.complexity_metrics.is_empty();
        saw_annotations |= annotation_count(&full) > 0;
    }
    assert!(
        saw_complexity && saw_annotations,
        "the fixtures must produce complexity metrics and symbol annotations at full level, \
         or the lower-level emptiness assertions are vacuous \
         (complexity={saw_complexity}, annotations={saw_annotations})"
    );
}
