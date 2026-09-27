use std::collections::BTreeSet;
use std::path::Path;

use julie_extractors::{ExtractionLevel, ExtractionResults, extract_canonical_for_language_at};

fn extract(language: &str, path: &str, source: &str) -> ExtractionResults {
    extract_canonical_for_language_at(
        language,
        path,
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
}

#[test]
fn swift_testing_traits_keep_direct_expressions_and_annotated_owners() {
    let source = include_str!("../../../fixtures/extraction/swift/testing_traits/source.swift");
    let results = extract("swift", "testing_traits.swift", source);
    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "swift_testing.trait.v1")
        .collect();

    let expected = [
        (
            "SwiftTraitSuite",
            "tags",
            ".tags(.slow, .network)",
            &[".slow", ".network"][..],
        ),
        ("SwiftTraitSuite", "serialized", ".serialized", &[][..]),
        (
            "SwiftTraitSuite",
            "timeLimit",
            ".timeLimit(.minutes(1))",
            &[".minutes(1)"][..],
        ),
        (
            "disabledWithDisplayName",
            "disabled",
            ".disabled(\"flaky\")",
            &["\"flaky\""][..],
        ),
        (
            "disabledConditionally",
            "disabled",
            ".disabled(if: flag)",
            &["if: flag"][..],
        ),
        (
            "enabledConditionally",
            "enabled",
            ".enabled(if: featureIsReady)",
            &["if: featureIsReady"][..],
        ),
        ("serialized", "serialized", ".serialized", &[][..]),
        (
            "limited",
            "timeLimit",
            ".timeLimit(.minutes(1))",
            &[".minutes(1)"][..],
        ),
    ];

    assert_eq!(facts.len(), expected.len());
    let observed: BTreeSet<_> = facts
        .iter()
        .map(|fact| {
            let owner = results
                .symbols
                .iter()
                .find(|symbol| Some(symbol.id.as_str()) == fact.containing_symbol_id.as_deref())
                .map(|symbol| symbol.name.as_str())
                .unwrap_or("");
            (
                owner.to_string(),
                fact.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("trait"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                source
                    .get(fact.start_byte as usize..fact.end_byte as usize)
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect();
    let expected_spans: BTreeSet<_> = expected
        .iter()
        .map(|(owner, trait_name, span, _)| {
            (
                (*owner).to_string(),
                (*trait_name).to_string(),
                (*span).to_string(),
            )
        })
        .collect();
    assert_eq!(observed, expected_spans);

    for (owner, trait_name, span, expected_arguments) in expected {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == owner)
            .unwrap();
        let fact = facts
            .iter()
            .find(|fact| {
                fact.containing_symbol_id.as_deref() == Some(symbol.id.as_str())
                    && fact
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("trait"))
                        .and_then(serde_json::Value::as_str)
                        == Some(trait_name)
                    && source.get(fact.start_byte as usize..fact.end_byte as usize) == Some(span)
            })
            .unwrap();
        assert_eq!(
            fact.containing_symbol_id.as_deref(),
            Some(symbol.id.as_str())
        );
        let metadata = fact.metadata.as_ref().unwrap();
        assert_eq!(
            metadata
                .get("query_family")
                .and_then(serde_json::Value::as_str),
            Some("testing")
        );
        assert_eq!(
            metadata
                .get("framework")
                .and_then(serde_json::Value::as_str),
            Some("swift_testing")
        );
        let arguments: Vec<_> = metadata
            .get("arguments")
            .and_then(serde_json::Value::as_array)
            .map(|arguments| {
                arguments
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(arguments, expected_arguments);
    }

    let disabled = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "disabledWithDisplayName")
        .unwrap();
    assert_eq!(
        disabled
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("is_test"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(disabled.annotations.iter().any(|annotation| {
        annotation.raw_text.as_deref()
            == Some("Test(\"café\", .disabled(\"flaky\"), arguments: [1, 2])")
    }));
    let suite = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "SwiftTraitSuite")
        .unwrap();
    assert_eq!(
        suite
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_container"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn swift_custom_test_identifier_without_testing_import_emits_no_traits() {
    let source = include_str!(
        "../../../fixtures/extraction/swift/testing_traits/custom_test_identifier.swift"
    );
    let results = extract("swift", "custom_test_identifier.swift", source);

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.pattern_id != "swift_testing.trait.v1")
    );
}

#[test]
fn rust_benchmark_attributes_and_criterion_registrations_are_source_backed() {
    let source = include_str!("../../../fixtures/extraction/rust/benchmark_harnesses/source.rs");
    let results = extract("rust", "benchmark_harnesses.rs", source);
    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "rust.benchmark.v1")
        .collect();

    assert_eq!(facts.len(), 5);
    let mut observed = BTreeSet::new();
    for fact in &facts {
        let metadata = fact.metadata.as_ref().unwrap();
        assert_eq!(
            metadata
                .get("query_family")
                .and_then(serde_json::Value::as_str),
            Some("testing")
        );
        let registration_kind = metadata
            .get("registration_kind")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let harness = metadata
            .get("harness")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let source_span = source
            .get(fact.start_byte as usize..fact.end_byte as usize)
            .unwrap();
        observed.insert((registration_kind, harness, source_span));
    }
    assert_eq!(
        observed,
        BTreeSet::from([
            ("function_attribute", "libtest", "#[bench]"),
            (
                "function_attribute",
                "divan",
                "#[divan::bench(args = [1, 2], sample_count = 100)]"
            ),
            (
                "group_registration",
                "criterion",
                "criterion_group!(criterion_benches, criterion_fast, criterion_slow)"
            ),
            (
                "group_registration",
                "criterion",
                "criterion_group! {\n    name = criterion_complete;\n    config = Criterion::default();\n    targets = criterion_slow, criterion_fast\n}"
            ),
            (
                "entry_point_registration",
                "criterion",
                "criterion_main!(criterion_benches, criterion_complete)"
            ),
        ])
    );

    for (source_name, expected_harness) in [
        ("libtest_benchmark", "libtest"),
        ("divan_benchmark", "divan"),
    ] {
        let fact = facts
            .iter()
            .find(|fact| {
                fact.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("target_name"))
                    .and_then(serde_json::Value::as_str)
                    == Some(source_name)
            })
            .unwrap();
        let metadata = fact.metadata.as_ref().unwrap();
        assert_eq!(
            metadata.get("harness").and_then(serde_json::Value::as_str),
            Some(expected_harness)
        );
        assert!(results.symbols.iter().any(|symbol| {
            Some(symbol.id.as_str()) == fact.containing_symbol_id.as_deref()
                && symbol.name == source_name
        }));
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == source_name)
            .unwrap();
        assert_ne!(
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("is_test"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test_role"))
                .is_none()
        );
    }

    let ordinary_test = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "ordinary_test")
        .unwrap();
    assert_eq!(
        ordinary_test
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("is_test"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let compact_group = facts
        .iter()
        .find(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("registration_kind"))
                .and_then(serde_json::Value::as_str)
                == Some("group_registration")
        })
        .unwrap();
    let group_metadata = compact_group.metadata.as_ref().unwrap();
    assert_eq!(
        group_metadata
            .get("group_name")
            .and_then(serde_json::Value::as_str),
        Some("criterion_benches")
    );
    let targets: Vec<_> = group_metadata
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(targets, ["criterion_fast", "criterion_slow"]);

    let complete_group = facts
        .iter()
        .find(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("group_name"))
                .and_then(serde_json::Value::as_str)
                == Some("criterion_complete")
        })
        .unwrap();
    let complete_targets: Vec<_> = complete_group
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("targets"))
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(complete_targets, ["criterion_slow", "criterion_fast"]);

    let entry_point = facts
        .iter()
        .find(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("registration_kind"))
                .and_then(serde_json::Value::as_str)
                == Some("entry_point_registration")
        })
        .unwrap();
    let entry_targets: Vec<_> = entry_point
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("targets"))
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(entry_targets, ["criterion_benches", "criterion_complete"]);
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.name != "criterion_benches")
    );
}

#[test]
fn rust_criterion_macros_require_import_or_qualified_evidence() {
    let source =
        include_str!("../../../fixtures/extraction/rust/benchmark_harnesses/unrelated_macros.rs");
    let results = extract("rust", "unrelated_macros.rs", source);

    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "rust.benchmark.v1")
        .collect();
    let spans: BTreeSet<_> = facts
        .iter()
        .filter_map(|fact| source.get(fact.start_byte as usize..fact.end_byte as usize))
        .collect();

    assert_eq!(
        spans,
        BTreeSet::from([
            "criterion::criterion_group!(qualified_group, benchmark_target)",
            "criterion::criterion_main!(qualified_group)",
        ])
    );
}

#[test]
fn rust_local_macro_definition_shadows_imported_criterion_macro() {
    let source =
        include_str!("../../../fixtures/extraction/rust/benchmark_harnesses/local_macro_shadow.rs");
    let results = extract("rust", "local_macro_shadow.rs", source);
    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "rust.benchmark.v1")
        .collect();

    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| {
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("registration_kind"))
            .and_then(serde_json::Value::as_str)
            == Some("group_registration")
    }));
    assert!(facts.iter().any(|fact| {
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("registration_kind"))
            .and_then(serde_json::Value::as_str)
            == Some("entry_point_registration")
    }));
    assert!(!facts.iter().any(|fact| {
        source.get(fact.start_byte as usize..fact.end_byte as usize)
            == Some("criterion_main!(shadowed_group)")
    }));
}
