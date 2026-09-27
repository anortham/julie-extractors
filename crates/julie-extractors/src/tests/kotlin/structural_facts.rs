use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/basic/source.kt");
const KOTEST_CHECKS_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/kotest_checks/source.kt");
const KOTEST_UNRELATED_CALLS_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/kotest_checks/unrelated_calls.kt");
const KOTEST_SHADOWED_IMPORT_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/kotest_checks/shadowed_import.kt");
const KOTEST_WILDCARD_AND_QUALIFIED_SOURCE: &str = include_str!(
    "../../../../../fixtures/extraction/kotlin/kotest_checks/wildcard_and_qualified.kt"
);
const KOTEST_SHADOWED_BINDINGS_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/kotest_checks/shadowed_bindings.kt");

fn extract(source: &str) -> crate::ExtractionResults {
    extract_at("fixtures/extraction/kotlin/basic/source.kt", source)
}

fn extract_at(path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(path, source, Path::new("/repo"))
        .expect("canonical Kotlin extraction should succeed")
}

#[test]
fn kotlin_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "kotlin.suspend_modifier.v1",
        "kotlin.property_delegate.v1",
        "kotlin.annotation.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let suspend = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "kotlin.suspend_modifier.v1")
        .expect("expected suspend modifier fact");
    assert_eq!(suspend.node_kind, "suspend");
    assert_eq!(metadata_str(suspend, "query_family"), Some("async"));
    assert!(suspend.containing_symbol_id.is_some());

    let delegate = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "kotlin.property_delegate.v1")
        .expect("expected property delegate fact");
    assert_eq!(metadata_str(delegate, "delegate_name"), Some("lazy"));
}

#[test]
fn kotest_checks_emit_facts_on_the_named_test_without_expanding_runtime_cases() {
    let path = "fixtures/extraction/kotlin/kotest_checks/source.kt";
    let results = extract_at(path, KOTEST_CHECKS_SOURCE);
    let facts = results
        .structural_facts
        .iter()
        .filter(|fact| {
            matches!(
                fact.pattern_id.as_str(),
                "kotest.table_check.v1" | "kotest.property_check.v1"
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(facts.len(), 5);
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.pattern_id == "kotest.table_check.v1")
            .count(),
        2
    );
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.pattern_id == "kotest.property_check.v1")
            .count(),
        3
    );

    let named_test = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "checks")
        .expect("expected named test function");
    assert_eq!(
        named_test
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .and_then(|value| value.as_str()),
        Some("test_case")
    );

    for fact in &facts {
        assert_eq!(metadata_str(fact, "query_family"), Some("testing"));
        assert_eq!(metadata_str(fact, "framework"), Some("kotest"));
        assert_eq!(
            fact.containing_symbol_id.as_deref(),
            Some(named_test.id.as_str())
        );
        let expression = &KOTEST_CHECKS_SOURCE[fact.start_byte as usize..fact.end_byte as usize];
        let callee = metadata_str(fact, "callee").expect("checks should retain their callee");
        assert!(expression.starts_with(callee));
    }

    let table = facts
        .iter()
        .find(|fact| metadata_str(fact, "callee") == Some("forAllRows"))
        .expect("expected aliased table check");
    let table_arguments = table
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("arguments"))
        .and_then(|value| value.as_array())
        .expect("table arguments should be retained");
    assert_eq!(
        table_arguments
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        ["row(2, 4)", "row(3, 9)"]
    );

    let multi_type_check = facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "kotest.property_check.v1"
                && fact
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("type_arguments"))
                    .and_then(|value| value.as_array())
                    .is_some_and(|arguments| arguments.len() == 2)
        })
        .expect("multi-type property check should keep both type arguments");
    let type_arguments = multi_type_check
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("type_arguments"))
        .and_then(|value| value.as_array())
        .expect("property type arguments should be retained");
    assert_eq!(
        type_arguments
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        ["Int", "String"]
    );

    let parameterized_cases = results
        .symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("test_role"))
                    .and_then(|value| value.as_str()),
                Some("test_case" | "parameterized_test")
            )
        })
        .count();
    assert_eq!(parameterized_cases, 2);
    assert!(results.symbols.iter().any(|symbol| {
        symbol.name == "withData"
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test_role"))
                .and_then(|value| value.as_str())
                == Some("parameterized_test")
    }));
    assert!(!results.symbols.iter().any(|symbol| {
        matches!(
            symbol.name.as_str(),
            "forAllRows" | "forNone" | "checkAll" | "forAll"
        ) && symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .is_some()
    }));
}

#[test]
fn local_calls_named_like_kotest_checks_do_not_emit_framework_facts() {
    let results = extract_at(
        "fixtures/extraction/kotlin/kotest_checks/unrelated_calls.kt",
        KOTEST_UNRELATED_CALLS_SOURCE,
    );

    assert!(!results.structural_facts.iter().any(|fact| {
        matches!(
            fact.pattern_id.as_str(),
            "kotest.table_check.v1" | "kotest.property_check.v1"
        )
    }));
}

#[test]
fn local_function_declarations_shadow_kotest_imports() {
    let results = extract_at(
        "fixtures/extraction/kotlin/kotest_checks/shadowed_import.kt",
        KOTEST_SHADOWED_IMPORT_SOURCE,
    );

    assert!(!results.structural_facts.iter().any(|fact| {
        matches!(
            fact.pattern_id.as_str(),
            "kotest.table_check.v1" | "kotest.property_check.v1"
        )
    }));
}

#[test]
fn kotest_wildcard_and_qualified_calls_emit_testing_facts() {
    let results = extract_at(
        "fixtures/extraction/kotlin/kotest_checks/wildcard_and_qualified.kt",
        KOTEST_WILDCARD_AND_QUALIFIED_SOURCE,
    );
    let facts = results
        .structural_facts
        .iter()
        .filter(|fact| {
            matches!(
                fact.pattern_id.as_str(),
                "kotest.table_check.v1" | "kotest.property_check.v1"
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| {
        fact.pattern_id == "kotest.table_check.v1" && metadata_str(fact, "callee") == Some("forAll")
    }));
    assert!(facts.iter().any(|fact| {
        fact.pattern_id == "kotest.property_check.v1"
            && metadata_str(fact, "callee") == Some("io.kotest.property.checkAll")
    }));
}

#[test]
fn local_bindings_shadow_kotest_imports_only_in_their_scope() {
    let results = extract_at(
        "fixtures/extraction/kotlin/kotest_checks/shadowed_bindings.kt",
        KOTEST_SHADOWED_BINDINGS_SOURCE,
    );
    let facts = results
        .structural_facts
        .iter()
        .filter(|fact| {
            matches!(
                fact.pattern_id.as_str(),
                "kotest.table_check.v1" | "kotest.property_check.v1"
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| {
        fact.pattern_id == "kotest.property_check.v1"
            && KOTEST_SHADOWED_BINDINGS_SOURCE[fact.start_byte as usize..fact.end_byte as usize]
                .starts_with("checkAll<Int>")
    }));
    assert!(facts.iter().any(|fact| {
        fact.pattern_id == "kotest.property_check.v1"
            && KOTEST_SHADOWED_BINDINGS_SOURCE[fact.start_byte as usize..fact.end_byte as usize]
                .starts_with("forAll<Int>")
    }));
}
