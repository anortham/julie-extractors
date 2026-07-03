//! Conformance between emitted structural facts and the structural-fact pattern
//! registry (`crate::base::structural_fact_registry`).
//!
//! The registry is the machine-readable contract for structural-fact metadata.
//! This module proves emission actually honours that contract: for every
//! structural fact the canonical extractor produces over the whole golden
//! fixture corpus,
//!
//! 1. the fact's `pattern_id` is declared in the registry,
//! 2. every metadata key the fact carries is declared with a matching value
//!    type, and
//! 3. every `Always` key the spec declares is present.
//!
//! The registry may declare `Optional` keys the corpus never exercises, but a
//! declared `Always` key is never absent and an undeclared key is never present.
//!
//! Gating: the corpus-walking conformance test runs canonical extraction over
//! the full fixture tree, so it is gated behind `test-golden`. The module itself
//! is registered ungated in `tests/mod.rs` so a default-suite sync test (Task 3)
//! can share this file without a module-level gating conflict.

#[cfg(feature = "test-golden")]
mod golden_corpus {
    use crate::base::{
        KeyPresence, MetadataValueType, StructuralFact, StructuralFactPatternSpec,
        structural_fact_pattern_specs,
    };
    use crate::extract_canonical;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    /// Repository root, derived from this crate's manifest dir (crate lives at
    /// `<root>/crates/julie-extractors`). Mirrors the golden test harness.
    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("julie-extractors crate should live under crates/")
            .to_path_buf()
    }

    /// The capability matrix; authoritative list of every language and fixture.
    fn load_matrix(root: &Path) -> Value {
        let matrix_path = root.join("fixtures/extraction/capabilities.json");
        let json = std::fs::read_to_string(&matrix_path).unwrap_or_else(|err| {
            panic!(
                "failed to read capability matrix at {}: {err}",
                matrix_path.display()
            )
        });
        serde_json::from_str(&json).unwrap_or_else(|err| {
            panic!(
                "failed to parse capability matrix at {}: {err}",
                matrix_path.display()
            )
        })
    }

    /// Whether a JSON value satisfies a declared registry value type.
    fn value_matches(value: &Value, expected: MetadataValueType) -> bool {
        match expected {
            MetadataValueType::String => value.is_string(),
            MetadataValueType::Bool => value.is_boolean(),
            MetadataValueType::Number => value.is_number(),
            MetadataValueType::StringArray => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            MetadataValueType::ObjectArray => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_object)),
        }
    }

    /// Human-readable JSON type name for failure messages.
    fn observed_type(value: &Value) -> &'static str {
        match value {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// Check one emitted fact against the registry, recording every violation.
    /// `violations` is a set so the same drift across many fixtures collapses to
    /// one message per distinct (pattern, key, fixture) tuple.
    fn check_fact(
        fact: &StructuralFact,
        specs: &BTreeMap<&str, &StructuralFactPatternSpec>,
        fixture: &str,
        violations: &mut BTreeSet<String>,
    ) {
        let pattern_id = fact.pattern_id.as_str();

        // (1) pattern_id must be declared.
        let Some(spec) = specs.get(pattern_id) else {
            violations.insert(format!(
                "pattern `{pattern_id}` is emitted (fixture {fixture}) but is not declared in the registry"
            ));
            return;
        };

        let declared: BTreeMap<&str, MetadataValueType> = spec
            .metadata_keys
            .iter()
            .map(|meta| (meta.key, meta.value_type))
            .collect();

        let metadata = fact.metadata.as_ref();

        // (2) every emitted key must be declared, with a matching value type.
        if let Some(metadata) = metadata {
            for (key, value) in metadata {
                let Some(&value_type) = declared.get(key.as_str()) else {
                    violations.insert(format!(
                        "pattern `{pattern_id}` (fixture {fixture}) emits undeclared metadata key `{key}`"
                    ));
                    continue;
                };
                if !value_matches(value, value_type) {
                    violations.insert(format!(
                        "pattern `{pattern_id}` (fixture {fixture}) metadata key `{key}` is declared as {value_type:?} but emitted a {} value",
                        observed_type(value)
                    ));
                }
            }
        }

        // (3) every Always key must be present.
        for meta in spec.metadata_keys {
            if meta.presence == KeyPresence::Always {
                let present = metadata.is_some_and(|m| m.contains_key(meta.key));
                if !present {
                    violations.insert(format!(
                        "pattern `{pattern_id}` (fixture {fixture}) is missing declared Always metadata key `{}`",
                        meta.key
                    ));
                }
            }
        }
    }

    /// Conformance rule over the full golden fixture corpus: every emitted
    /// structural fact matches its registry spec (pattern declared, keys
    /// declared with matching value types, all `Always` keys present).
    #[test]
    fn structural_facts_conform_to_registry() {
        let root = workspace_root();
        let matrix = load_matrix(&root);

        let specs: BTreeMap<&str, &StructuralFactPatternSpec> = structural_fact_pattern_specs()
            .iter()
            .map(|spec| (spec.pattern_id, spec))
            .collect();

        let languages = matrix["languages"]
            .as_array()
            .expect("capabilities.json must have a `languages` array");

        let mut violations: BTreeSet<String> = BTreeSet::new();
        let mut fact_count: usize = 0;

        for row in languages {
            let language = row["language"]
                .as_str()
                .expect("capability row `language` must be a string");
            let fixtures = row["fixtures"]
                .as_array()
                .unwrap_or_else(|| panic!("{language} `fixtures` must be an array"));

            for fixture in fixtures {
                let source_path = fixture["source"].as_str().unwrap_or_else(|| {
                    panic!("{language} fixture entries must include a string `source` path")
                });
                let source =
                    std::fs::read_to_string(root.join(source_path)).unwrap_or_else(|err| {
                        panic!("failed to read fixture source {source_path}: {err}")
                    });
                let results =
                    extract_canonical(source_path, &source, &root).unwrap_or_else(|err| {
                        panic!(
                            "extract_canonical failed for {language} fixture {source_path}: {err}"
                        )
                    });

                for fact in &results.structural_facts {
                    fact_count += 1;
                    check_fact(fact, &specs, source_path, &mut violations);
                }
            }
        }

        assert!(
            fact_count > 0,
            "conformance test observed zero structural facts across the golden corpus — \
             fixture discovery is broken"
        );

        assert!(
            violations.is_empty(),
            "structural-fact registry conformance failed ({} distinct violation(s)):\n{}",
            violations.len(),
            violations.iter().cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

// ---------------------------------------------------------------------------
// Ungated checked-in-JSON sync test (Task 3).
//
// The registry is published as a checked-in JSON contract at
// `docs/contracts/structural-fact-patterns.json` so downstream consumers
// (Miller and others) can vendor/pin the metadata-payload shape without linking
// the Rust crate. This test proves the artifact never drifts from the
// serializer. It only serializes the in-memory registry and reads one file, so
// it is a sub-second default-suite test — unlike the golden-corpus conformance
// test above, it is intentionally NOT gated behind `test-golden`.
//
// Regenerate the artifact after an intentional registry change with:
//   UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry
// ---------------------------------------------------------------------------

/// Repository root, derived from this crate's manifest dir. Mirrors the helper
/// inside `golden_corpus`, but lives at file scope so the ungated sync test can
/// use it whether or not `test-golden` is enabled.
fn contract_workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("julie-extractors crate should live under crates/")
        .to_path_buf()
}

/// The checked-in JSON contract must byte-match the registry serializer that
/// produces it (the same function Task 4 embeds in `languages --json`).
#[test]
fn structural_fact_patterns_json_matches_checked_in_contract() {
    let contract_path =
        contract_workspace_root().join("docs/contracts/structural-fact-patterns.json");
    let generated = crate::base::structural_fact_patterns_contract_json();

    // Regeneration path: rewrite the artifact from the registry and stop.
    if std::env::var_os("UPDATE_CONTRACT_JSON").is_some() {
        if let Some(parent) = contract_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&contract_path, &generated).unwrap_or_else(|err| {
            panic!(
                "failed to write structural-fact contract artifact at {}: {err}",
                contract_path.display()
            )
        });
        return;
    }

    let checked_in = std::fs::read_to_string(&contract_path).unwrap_or_else(|err| {
        panic!(
            "missing structural-fact contract artifact at {}: {err}. \
             Regenerate with `UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`",
            contract_path.display()
        )
    });

    assert_eq!(
        checked_in, generated,
        "docs/contracts/structural-fact-patterns.json is out of sync with the structural-fact \
         pattern registry. Regenerate with \
         `UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`."
    );
}

#[test]
fn markdown_contract_pattern_tables_list_web_markup_pattern_rows() {
    let root = contract_workspace_root();
    let docs = [
        root.join("docs/contracts/jsonl-v3.md"),
        root.join("docs/contracts/sqlite-schema-v3.md"),
    ];

    let expected = [
        "razor.page_directive.v1",
        "razor.code_block.v1",
        "razor.template_expression.v1",
        "css.selector_rule.v1",
        "css.custom_property.v1",
        "css.media_query.v1",
        "css.keyframes.v1",
        "html.link.v1",
        "html.script.v1",
        "html.form.v1",
        "html.form_control.v1",
        "vue.sfc_section.v1",
        "vue.template_directive.v1",
    ];

    for doc in docs {
        let content = std::fs::read_to_string(&doc)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", doc.display()));
        let table_rows = content
            .lines()
            .filter(|line| line.starts_with("| `"))
            .collect::<Vec<_>>()
            .join("\n");
        let missing = expected
            .iter()
            .filter(|pattern_id| !table_rows.contains(&format!("`{pattern_id}`")))
            .copied()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "{} structural-fact pattern table is missing registered pattern row(s): {}",
            doc.display(),
            missing.join(", ")
        );
    }
}
