use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.toml", source, Path::new("/repo"))
        .expect("canonical TOML extraction should succeed")
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

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

#[test]
fn toml_emits_table_key_value_and_inline_table_facts() {
    let source = r#"
[worker]
id = 1
name = "fixture"
profile = { role = "admin", active = true }

[[items]]
key = "alpha"
"#;

    let results = extract(source);

    let tables = facts_with_pattern(&results, "toml.table.v1");
    assert!(
        tables
            .iter()
            .any(|fact| metadata_str(fact, "table_name") == Some("worker"))
    );
    assert!(
        tables
            .iter()
            .all(|fact| metadata_bool(fact, "is_array_table") == Some(false))
    );

    let array_tables = facts_with_pattern(&results, "toml.array_table.v1");
    assert_eq!(array_tables.len(), 1);
    assert_eq!(metadata_str(array_tables[0], "table_name"), Some("items"));
    assert_eq!(metadata_bool(array_tables[0], "is_array_table"), Some(true));

    let key_values = facts_with_pattern(&results, "toml.key_value.v1");
    assert_eq!(
        key_values
            .iter()
            .filter_map(|fact| metadata_str(fact, "key"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["active", "id", "key", "name", "profile", "role"])
    );
    assert!(
        key_values
            .iter()
            .any(|fact| metadata_str(fact, "value_kind") == Some("inline_table"))
    );

    let inline_tables = facts_with_pattern(&results, "toml.inline_table.v1");
    assert_eq!(inline_tables.len(), 1);
    assert_eq!(
        metadata_str(inline_tables[0], "key_path"),
        Some("worker.profile")
    );

    assert!(key_values.iter().any(|fact| {
        metadata_str(fact, "key") == Some("role")
            && metadata_str(fact, "key_path") == Some("worker.profile.role")
    }));
    assert!(key_values.iter().any(|fact| {
        metadata_str(fact, "key") == Some("active")
            && metadata_str(fact, "key_path") == Some("worker.profile.active")
    }));
}

#[test]
fn toml_dotted_keys_and_arrays_of_inline_tables_preserve_paths() {
    let source = r#"
database.settings.timeout = 30

[service]
routes = [{ path = "/health", method = "GET" }, { path = "/ready", method = "POST" }]
"#;

    let results = extract(source);
    let key_values = facts_with_pattern(&results, "toml.key_value.v1");
    assert!(key_values.iter().any(|fact| {
        metadata_str(fact, "key") == Some("timeout")
            && metadata_str(fact, "key_path") == Some("database.settings.timeout")
    }));

    let paths = key_values
        .iter()
        .filter(|fact| metadata_str(fact, "key") == Some("path"))
        .filter_map(|fact| metadata_str(fact, "key_path"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        paths,
        BTreeSet::from(["service.routes[0].path", "service.routes[1].path"])
    );

    let inline_tables = facts_with_pattern(&results, "toml.inline_table.v1");
    let inline_paths = inline_tables
        .iter()
        .filter_map(|fact| metadata_str(fact, "key_path"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        inline_paths,
        BTreeSet::from(["service.routes[0]", "service.routes[1]"])
    );
}
