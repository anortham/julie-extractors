use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/swift/basic/source.swift");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/swift/basic/source.swift",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Swift extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn swift_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "swift.await_expression.v1",
        "swift.actor_declaration.v1",
        "swift.attribute.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let actor = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "swift.actor_declaration.v1")
        .expect("expected actor declaration fact");
    assert_eq!(metadata_str(actor, "actor_name"), Some("Counter"));
    assert_eq!(actor.node_kind, "class_declaration");
    assert_eq!(metadata_str(actor, "query_family"), Some("concurrency"));

    let await_fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "swift.await_expression.v1")
        .expect("expected await expression fact");
    assert_eq!(await_fact.node_kind, "await_expression");
}
