use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/kotlin/basic/source.kt");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/kotlin/basic/source.kt",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Kotlin extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
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
