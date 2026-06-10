use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.yaml", source, Path::new("/repo"))
        .expect("canonical YAML extraction should succeed")
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
fn yaml_emits_document_mapping_sequence_anchor_and_alias_facts() {
    let source = r#"defaults: &defaults
  active: true

worker:
  <<: *defaults
  id: 1
  tags:
    - fixture
    - active
"#;

    let results = extract(source);

    assert_eq!(facts_with_pattern(&results, "yaml.document.v1").len(), 1);

    let mappings = facts_with_pattern(&results, "yaml.mapping.v1");
    assert!(mappings.len() >= 2);
    assert!(
        mappings
            .iter()
            .any(|fact| metadata_u64(fact, "pair_count").unwrap_or(0) >= 1)
    );

    let sequences = facts_with_pattern(&results, "yaml.sequence.v1");
    assert_eq!(sequences.len(), 1);
    assert_eq!(metadata_u64(sequences[0], "sequence_length"), Some(2));

    let anchor = facts_with_pattern(&results, "yaml.anchor.v1")
        .into_iter()
        .next()
        .expect("expected anchor fact");
    assert_eq!(metadata_str(anchor, "anchor_name"), Some("defaults"));

    let alias = facts_with_pattern(&results, "yaml.alias.v1")
        .into_iter()
        .next()
        .expect("expected alias fact");
    assert_eq!(metadata_str(alias, "alias_target"), Some("defaults"));
}
