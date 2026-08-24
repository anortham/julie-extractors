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
        if !spec.languages.contains(&fact.language.as_str()) {
            violations.insert(format!(
                "pattern `{pattern_id}` is emitted for language `{}` (fixture {fixture}) but the registry declares {:?}",
                fact.language, spec.languages
            ));
        }

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
                for source_path in fixture_source_paths(fixture, language) {
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

    fn fixture_source_paths<'a>(fixture: &'a Value, language: &str) -> Vec<&'a str> {
        let source = fixture["source"].as_str().unwrap_or_else(|| {
            panic!("{language} fixture entries must include a string `source` path")
        });
        let Some(sources) = fixture.get("sources") else {
            return vec![source];
        };
        let sources = sources
            .as_array()
            .unwrap_or_else(|| panic!("{language} fixture `sources` must be an array"));
        if sources.is_empty() {
            return vec![source];
        }
        let source_paths = sources
            .iter()
            .map(|value| {
                value.as_str().unwrap_or_else(|| {
                    panic!("{language} fixture `sources` values must be strings")
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            source_paths.first().copied(),
            Some(source),
            "{language} fixture must list source as sources[0]"
        );
        let mut seen = BTreeSet::new();
        for source_path in &source_paths {
            assert!(
                seen.insert(*source_path),
                "{language} fixture lists duplicate source {source_path}"
            );
        }
        source_paths
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

#[test]
fn vue_embedded_css_emissions_are_registered_for_vue() {
    let source = r#"<style>
@charset "UTF-8";
@namespace url(http://www.w3.org/1999/xhtml);
:root { --accent: #0f766e; }
@media (min-width: 40rem) { .wide { display: block; } }
@keyframes spin { from { opacity: 0; } to { opacity: 1; } }
@supports (display: grid) { .grid { display: grid; } }
@container (min-width: 20rem) { .card { color: red; } }
@font-face { font-family: "Worker"; src: url("/worker.woff2"); }
@layer utilities { .m-0 { margin: 0; } }
</style>
"#;
    let results =
        crate::pipeline::extract_canonical("source.vue", source, std::path::Path::new("/repo"))
            .expect("canonical Vue extraction should succeed");
    let emitted = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id.starts_with("css."))
        .map(|fact| fact.pattern_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let expected = std::collections::BTreeSet::from([
        "css.charset.v1",
        "css.container.v1",
        "css.custom_property.v1",
        "css.font_face.v1",
        "css.keyframes.v1",
        "css.layer.v1",
        "css.media_query.v1",
        "css.namespace.v1",
        "css.selector_rule.v1",
        "css.supports.v1",
    ]);
    assert_eq!(emitted, expected);

    let registered = crate::base::structural_fact_pattern_specs()
        .iter()
        .filter(|spec| spec.languages.contains(&"vue"))
        .map(|spec| spec.pattern_id)
        .collect::<std::collections::BTreeSet<_>>();
    let missing = emitted.difference(&registered).copied().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "Vue embedded CSS patterns missing from the registry: {missing:?}"
    );
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
fn blazor_generic_arguments_contract_describes_advisory_syntax_evidence() {
    let component_reference = crate::base::structural_fact_pattern_specs()
        .iter()
        .find(|spec| spec.pattern_id == "blazor.component_reference.v1")
        .expect("Blazor component-reference registry entry");
    let generic_arguments = component_reference
        .metadata_keys
        .iter()
        .find(|key| key.key == "generic_arguments")
        .expect("generic_arguments metadata contract");

    assert!(generic_arguments.description.contains("candidate evidence"));
    assert!(
        generic_arguments
            .description
            .contains("not resolved generic semantics")
    );
}

#[test]
fn structural_fact_pattern_specs_preserve_framework_markup_order() {
    let pattern_ids: Vec<&str> = crate::base::structural_fact_pattern_specs()
        .iter()
        .map(|spec| spec.pattern_id)
        .collect();
    let htmx_index = pattern_ids
        .iter()
        .position(|pattern_id| *pattern_id == "htmx.attribute.v1")
        .expect("htmx pattern must be registered");

    assert_eq!(
        &pattern_ids[htmx_index..htmx_index + 4],
        [
            "htmx.attribute.v1",
            "alpine.directive.v1",
            "blazor.component_reference.v1",
            "razor.page_directive.v1",
        ]
    );
}

/// Line ceilings that genuinely constrain regrowth of the split registry.
///
/// `FAMILY_CEILING` is the post-split maximum family file (`data.rs`, ~600
/// lines) plus small headroom, which is stricter than the plan's 800 target; a
/// family that grows past it must split further. `MOD_CEILING` is the plan's
/// mod-file target: module files carry only declarations, types, helpers, and
/// serializers — never a SPECS table.
const REGISTRY_FAMILY_CEILING: usize = 700;
const REGISTRY_MOD_CEILING: usize = 400;

fn registry_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", dir.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            registry_rs_files(path.as_path(), out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn structural_fact_registry_is_split_into_family_modules() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/base/structural_fact_registry");
    assert!(
        root.join("mod.rs").is_file(),
        "expected directory module at {}",
        root.display()
    );
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/base/structural_fact_registry.rs")
            .exists(),
        "monolithic structural_fact_registry.rs must not return"
    );

    let mut files = Vec::new();
    registry_rs_files(&root, &mut files);

    let mut spec_families = 0usize;
    for path in &files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name");
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let lines = source.lines().count();

        match name {
            "mod.rs" => {
                assert!(
                    !source.contains("pub(super) const SPECS"),
                    "{} is a module file; SPECS tables belong in family modules",
                    path.display()
                );
                assert!(
                    lines <= REGISTRY_MOD_CEILING,
                    "{} has {lines} lines; module files must stay <= {REGISTRY_MOD_CEILING}",
                    path.display()
                );
            }
            "tests.rs" => {
                assert!(
                    lines <= REGISTRY_FAMILY_CEILING,
                    "{} has {lines} lines; keep it <= {REGISTRY_FAMILY_CEILING}",
                    path.display()
                );
            }
            _ => {
                assert!(
                    source.contains("pub(super) const SPECS"),
                    "{} must declare pub(super) const SPECS",
                    path.display()
                );
                assert!(
                    lines <= REGISTRY_FAMILY_CEILING,
                    "{} has {lines} lines; family SPECS modules must stay <= {REGISTRY_FAMILY_CEILING} (split further if needed)",
                    path.display()
                );
                spec_families += 1;
            }
        }
    }

    assert!(
        spec_families >= 6,
        "expected the registry to remain split across family modules, found {spec_families}"
    );
}

/// Backtick tokens inside a cell, e.g. `` `css`, `vue` `` -> {css, vue}.
fn backtick_tokens(cell: &str) -> std::collections::BTreeSet<String> {
    let mut tokens = std::collections::BTreeSet::new();
    let mut i = 0;
    while let Some(open) = cell[i..].find('`') {
        let start = i + open + 1;
        let Some(rel_close) = cell[start..].find('`') else {
            break;
        };
        let end = start + rel_close;
        tokens.insert(cell[start..end].to_string());
        i = end + 1;
    }
    tokens
}

/// Map every `| `pattern` | `langs` | …` row in a contract doc to its declared
/// language set (first column -> second column).
fn doc_pattern_language_rows(
    content: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut rows = std::collections::BTreeMap::new();
    for line in content.lines() {
        if !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        if cells.len() < 2 {
            continue;
        }
        let ids = backtick_tokens(cells[0]);
        if ids.len() != 1 {
            continue;
        }
        let pattern_id = ids.into_iter().next().unwrap();
        rows.entry(pattern_id)
            .or_insert_with(|| backtick_tokens(cells[1]));
    }
    rows
}

/// The live contract docs (JSONL v3 + SQLite schema v4) must carry a row for
/// every web-markup structural-fact pattern the registry declares, with a
/// matching language set. The css/html/vue expectation is DERIVED from the
/// registry so new patterns in those families are guarded automatically; the
/// three documented Razor markup rows are pinned explicitly.
///
/// (`sqlite-schema-v3.md` is frozen-historical — `docs/contracts/cli.md`
/// declares schema 4 — so it is intentionally no longer gated here.)
#[test]
fn markdown_contract_pattern_tables_list_web_markup_pattern_rows() {
    let root = contract_workspace_root();
    let docs = [
        root.join("docs/contracts/jsonl-v3.md"),
        root.join("docs/contracts/sqlite-schema-v4.md"),
    ];

    const RAZOR_MARKUP: &[&str] = &[
        "razor.page_directive.v1",
        "razor.code_block.v1",
        "razor.template_expression.v1",
    ];

    let specs = crate::base::structural_fact_pattern_specs();
    let expected: Vec<(&str, std::collections::BTreeSet<String>)> = specs
        .iter()
        .filter(|spec| {
            spec.pattern_id.starts_with("css.")
                || spec.pattern_id.starts_with("html.")
                || spec.pattern_id.starts_with("vue.")
                || RAZOR_MARKUP.contains(&spec.pattern_id)
        })
        .map(|spec| {
            (
                spec.pattern_id,
                spec.languages.iter().map(|l| l.to_string()).collect(),
            )
        })
        .collect();
    assert!(
        expected.len() > RAZOR_MARKUP.len(),
        "expected web-markup patterns to be derived from the registry"
    );

    for doc in docs {
        let content = std::fs::read_to_string(&doc)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", doc.display()));
        let rows = doc_pattern_language_rows(&content);

        for (pattern_id, registry_langs) in &expected {
            let Some(doc_langs) = rows.get(*pattern_id) else {
                panic!(
                    "{} structural-fact pattern table is missing the registered pattern row `{pattern_id}`",
                    doc.display()
                );
            };
            assert_eq!(
                doc_langs,
                registry_langs,
                "{} row for `{pattern_id}` lists languages {doc_langs:?} but the registry declares {registry_langs:?}",
                doc.display()
            );
        }
    }
}
