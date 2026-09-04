use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/scala/basic/source.scala");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/scala/basic/source.scala",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Scala extraction should succeed")
}


#[test]
fn scala_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "scala.extension_definition.v1",
        "scala.given_definition.v1",
        "scala.for_expression.v1",
        "scala.annotation.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let extension = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "scala.extension_definition.v1")
        .expect("expected extension definition fact");
    assert_eq!(metadata_str(extension, "extended_type"), Some("Int"));

    let given = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "scala.given_definition.v1")
        .expect("expected given definition fact");
    assert_eq!(metadata_str(given, "query_family"), Some("typeclass"));
    assert_eq!(metadata_str(given, "given_name"), None);
    assert_eq!(metadata_str(given, "given_type"), Some("Ordering[Int]"));
}

#[test]
fn scala_extension_extended_type_uses_receiver_not_return_type() {
    let source = r#"extension (value: String)
  def lengthHint: Int = value.length
"#;
    let results = extract(source);
    let extension = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "scala.extension_definition.v1")
        .expect("expected extension definition fact");
    assert_eq!(metadata_str(extension, "extended_type"), Some("String"));
}

#[test]
fn scala_named_given_emits_given_name_not_given_type() {
    let source = "given intOrdering: Ordering[Int] = Ordering.Int\n";
    let results = extract(source);
    let given = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "scala.given_definition.v1")
        .expect("expected given definition fact");
    assert_eq!(metadata_str(given, "given_name"), Some("intOrdering"));
    assert_eq!(metadata_str(given, "given_type"), None);
}
