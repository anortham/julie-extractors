use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/dart/basic/source.dart");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/dart/basic/source.dart",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Dart extraction should succeed")
}

#[test]
fn dart_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "dart.await_expression.v1",
        "dart.async_modifier.v1",
        "dart.annotation.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let await_fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "dart.await_expression.v1")
        .expect("expected await expression fact");
    assert_eq!(await_fact.node_kind, "await_expression");
    assert_eq!(metadata_str(await_fact, "query_family"), Some("async"));
    assert!(await_fact.containing_symbol_id.is_some());

    let override_annotation = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "dart.annotation.v1"
                && metadata_str(fact, "annotation_name") == Some("override")
        })
        .expect("expected @override annotation fact");
    assert_eq!(
        metadata_str(override_annotation, "query_family"),
        Some("metadata")
    );
}
