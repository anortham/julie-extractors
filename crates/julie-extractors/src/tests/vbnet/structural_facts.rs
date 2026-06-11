use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/vbnet/basic/source.vb");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/vbnet/basic/source.vb",
        source,
        Path::new("/repo"),
    )
    .expect("canonical VB.NET extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn vbnet_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "vbnet.handles_clause.v1",
        "vbnet.implements_clause.v1",
        "vbnet.event_declaration.v1",
        "vbnet.attribute.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let handles = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "vbnet.handles_clause.v1")
        .expect("expected handles clause fact");
    assert_eq!(
        metadata_str(handles, "handles_target"),
        Some("Button.Click")
    );

    let implements = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "vbnet.implements_clause.v1")
        .expect("expected implements clause fact");
    assert_eq!(metadata_str(implements, "implements_target"), Some("IJob"));
}
