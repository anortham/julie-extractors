use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.yaml", source, Path::new("/repo"))
        .expect("canonical YAML extraction should succeed")
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

    let worker_id = facts_with_pattern(&results, "yaml.key_value.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "key") == Some("id"))
        .expect("expected worker.id key-value fact");
    assert_eq!(metadata_str(worker_id, "key_path"), Some("$.worker.id"));
    assert_eq!(metadata_str(worker_id, "value_kind"), Some("scalar"));

    let worker_tags = facts_with_pattern(&results, "yaml.key_value.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "key") == Some("tags"))
        .expect("expected worker.tags key-value fact");
    assert_eq!(metadata_str(worker_tags, "key_path"), Some("$.worker.tags"));
    assert_eq!(metadata_str(worker_tags, "value_kind"), Some("sequence"));
}

#[test]
fn yaml_flow_collections_emit_paths_and_kinds() {
    let source = r#"
worker: { id: 1, tags: [fixture, active], profile: { role: admin } }
"#;

    let results = extract(source);
    let mappings = facts_with_pattern(&results, "yaml.mapping.v1");
    assert!(
        mappings
            .iter()
            .any(|fact| metadata_str(fact, "key_path") == Some("$.worker")),
        "{mappings:#?}"
    );

    let sequences = facts_with_pattern(&results, "yaml.sequence.v1");
    assert_eq!(sequences.len(), 1, "{sequences:#?}");
    assert_eq!(
        metadata_str(sequences[0], "key_path"),
        Some("$.worker.tags")
    );
    assert_eq!(metadata_u64(sequences[0], "sequence_length"), Some(2));

    let key_values = facts_with_pattern(&results, "yaml.key_value.v1");
    let id = key_values
        .iter()
        .find(|fact| metadata_str(fact, "key_path") == Some("$.worker.id"))
        .expect("flow id key");
    assert_eq!(metadata_str(id, "value_kind"), Some("scalar"));
    let tags = key_values
        .iter()
        .find(|fact| metadata_str(fact, "key_path") == Some("$.worker.tags"))
        .expect("flow tags key");
    assert_eq!(metadata_str(tags, "value_kind"), Some("sequence"));
    let role = key_values
        .iter()
        .find(|fact| metadata_str(fact, "key_path") == Some("$.worker.profile.role"))
        .expect("nested flow role key");
    assert_eq!(metadata_str(role, "value_kind"), Some("scalar"));
}
