use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.json", source, Path::new("/repo"))
        .expect("canonical JSON extraction should succeed")
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

#[test]
fn json_emits_object_property_and_array_facts_without_scalar_noise() {
    let source = r#"{
  "worker": {
    "id": 1,
    "tags": ["fixture", "active"],
    "active": true
  }
}"#;

    let results = extract(source);

    let objects = facts_with_pattern(&results, "json.object.v1");
    assert!(
        objects
            .iter()
            .any(|fact| metadata_str(fact, "path") == Some("$")),
        "expected root object fact"
    );
    assert!(
        objects
            .iter()
            .any(|fact| metadata_str(fact, "path") == Some("$.worker")),
        "expected nested object fact"
    );

    let properties = facts_with_pattern(&results, "json.property.v1");
    assert_eq!(
        properties
            .iter()
            .filter_map(|fact| metadata_str(fact, "key"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["active", "id", "tags", "worker"])
    );
    let worker_id = properties
        .iter()
        .find(|fact| metadata_str(fact, "key") == Some("id"))
        .expect("expected id property");
    assert_eq!(metadata_str(worker_id, "value_kind"), Some("number"));
    assert_eq!(metadata_str(worker_id, "path"), Some("$.worker"));

    let arrays = facts_with_pattern(&results, "json.array.v1");
    assert_eq!(arrays.len(), 1);
    assert_eq!(metadata_str(arrays[0], "path"), Some("$.worker.tags"));

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| !fact.pattern_id.ends_with(".string.v1"))
    );
}

#[test]
fn json_array_elements_have_unique_indexed_paths() {
    let source = r#"{
  "workers": [
    { "id": 1, "name": "a" },
    { "id": 2, "name": "b" }
  ]
}"#;

    let results = extract(source);
    let objects = facts_with_pattern(&results, "json.object.v1");
    assert!(
        objects
            .iter()
            .any(|fact| metadata_str(fact, "path") == Some("$.workers[0]")),
        "{objects:#?}"
    );
    assert!(
        objects
            .iter()
            .any(|fact| metadata_str(fact, "path") == Some("$.workers[1]")),
        "{objects:#?}"
    );

    let properties = facts_with_pattern(&results, "json.property.v1");
    let id_paths = properties
        .iter()
        .filter(|fact| metadata_str(fact, "key") == Some("id"))
        .filter_map(|fact| metadata_str(fact, "path"))
        .collect::<BTreeSet<_>>();
    assert_eq!(id_paths, BTreeSet::from(["$.workers[0]", "$.workers[1]"]));
}

#[test]
fn json_emits_schema_and_ref_structural_facts() {
    let source = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "properties": {
    "billing": { "$ref": "#/$defs/Address" },
    "shipping": { "$ref": "external.json#/$defs/Address" }
  }
}"##;
    let results = extract(source);

    let schema = facts_with_pattern(&results, "json.schema.v1")
        .into_iter()
        .next()
        .expect("expected $schema fact");
    assert_eq!(
        metadata_str(schema, "schema_uri"),
        Some("https://json-schema.org/draft/2020-12/schema")
    );
    assert_eq!(metadata_str(schema, "path"), Some("$"));
    assert_eq!(
        metadata_str(schema, "query_family"),
        Some("schema_structure")
    );

    let refs = facts_with_pattern(&results, "json.ref.v1");
    assert_eq!(refs.len(), 2);
    assert!(
        refs.iter()
            .any(|fact| metadata_str(fact, "ref") == Some("#/$defs/Address"))
    );
    assert!(refs.iter().any(|fact| {
        metadata_str(fact, "ref") == Some("external.json#/$defs/Address")
            && metadata_str(fact, "path") == Some("$.properties.shipping")
    }));
    assert!(
        refs.iter()
            .all(|fact| metadata_str(fact, "query_family") == Some("schema_structure"))
    );
}

#[test]
fn json_schema_and_ref_values_are_decoded() {
    let source = r##"{
  "$schema": "https:\/\/json-schema.org\/draft\/2020-12\/schema",
  "$ref": "#\/definitions\/caf\u00e9"
}"##;
    let results = extract(source);

    let schema = facts_with_pattern(&results, "json.schema.v1");
    assert_eq!(
        metadata_str(schema[0], "schema_uri"),
        Some("https://json-schema.org/draft/2020-12/schema")
    );

    let refs = facts_with_pattern(&results, "json.ref.v1");
    assert_eq!(metadata_str(refs[0], "ref"), Some("#/definitions/café"));
}
