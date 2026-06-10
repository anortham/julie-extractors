use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.regex", source, Path::new("/repo"))
        .expect("canonical regex extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

#[test]
fn regex_emits_capture_lookaround_class_quantifier_alternation_and_anchor_facts() {
    let source = r#"^(?<name>[A-Za-z]+)|(?<id>\d+)-\k<name>-(foo)-\3$"#;

    let results = extract(source);

    let named_captures = facts_with_pattern(&results, "regex.named_capture.v1");
    assert_eq!(
        named_captures
            .iter()
            .filter_map(|fact| metadata_str(fact, "capture_name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["id", "name"])
    );

    let capture_groups = facts_with_pattern(&results, "regex.capture_group.v1");
    assert!(
        capture_groups
            .iter()
            .any(|fact| metadata_u64(fact, "capture_index").is_some())
    );

    let anchors = facts_with_pattern(&results, "regex.anchor.v1");
    assert!(
        anchors
            .iter()
            .any(|fact| metadata_str(fact, "anchor_kind") == Some("start"))
    );
    assert!(
        anchors
            .iter()
            .any(|fact| metadata_str(fact, "anchor_kind") == Some("end"))
    );

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.end_byte > fact.start_byte)
    );
}

#[test]
fn regex_emits_character_class_and_alternation_facts() {
    let source = r#"[A-Za-z]+|foo|bar"#;

    let results = extract(source);

    let classes = facts_with_pattern(&results, "regex.character_class.v1");
    assert_eq!(classes.len(), 1);
    assert_eq!(
        metadata_str(classes[0], "query_family"),
        Some("pattern_structure")
    );

    let alternations = facts_with_pattern(&results, "regex.alternation.v1");
    assert!(!alternations.is_empty());
    assert!(
        alternations
            .iter()
            .any(|fact| metadata_u64(fact, "branch_count").unwrap_or(0) >= 2)
    );
}

#[test]
fn regex_quantifier_facts_cover_bounded_repetition() {
    let source = r#"a{2,4}"#;

    let results = extract(source);
    let quantifiers = facts_with_pattern(&results, "regex.quantifier.v1");
    assert!(!quantifiers.is_empty());
    assert!(
        quantifiers
            .iter()
            .any(|fact| metadata_str(fact, "quantifier").is_some_and(|q| q.contains('{')))
    );
}
