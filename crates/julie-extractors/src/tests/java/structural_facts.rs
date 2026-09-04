use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/java/basic/source.java");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/java/basic/source.java",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Java extraction should succeed")
}

#[test]
fn java_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "java.synchronized_statement.v1",
        "java.try_with_resources_statement.v1",
        "java.lambda_expression.v1",
        "java.marker_annotation.v1",
        "java.annotation.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let synchronized = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "java.synchronized_statement.v1")
        .expect("expected synchronized structural fact");
    assert_eq!(synchronized.capture_name, "synchronized_statement");
    assert_eq!(synchronized.node_kind, "synchronized_statement");
    assert_eq!(
        metadata_str(synchronized, "query_family"),
        Some("concurrency")
    );
    assert!(synchronized.containing_symbol_id.is_some());

    let deprecated = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "java.marker_annotation.v1"
                && metadata_str(fact, "annotation_name") == Some("Deprecated")
        })
        .expect("expected @Deprecated marker annotation fact");
    assert_eq!(metadata_str(deprecated, "query_family"), Some("metadata"));

    let suppress = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "java.annotation.v1"
                && metadata_str(fact, "annotation_name") == Some("SuppressWarnings")
        })
        .expect("expected @SuppressWarnings annotation fact");
    assert_eq!(suppress.node_kind, "annotation");
    assert_eq!(metadata_str(suppress, "query_family"), Some("metadata"));
}
