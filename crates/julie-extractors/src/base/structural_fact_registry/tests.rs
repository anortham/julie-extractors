//! Registry-invariant unit tests for the structural-fact pattern registry.
//!
//! These assert internal well-formedness of the SPECS table (unique ids,
//! non-empty fields, base-key declarations). Cross-artifact and conformance
//! tests live in `crate::tests::structural_fact_registry`.

use super::*;
use std::collections::HashSet;

#[test]
fn pattern_ids_are_unique() {
    let mut seen = HashSet::new();
    for spec in structural_fact_pattern_specs() {
        assert!(
            seen.insert(spec.pattern_id),
            "duplicate pattern_id in registry: {}",
            spec.pattern_id
        );
    }
}

#[test]
fn every_spec_has_nonempty_languages() {
    for spec in structural_fact_pattern_specs() {
        assert!(
            !spec.languages.is_empty(),
            "{} declares no languages",
            spec.pattern_id
        );
        for language in spec.languages {
            assert!(
                !language.is_empty(),
                "{} declares an empty language string",
                spec.pattern_id
            );
        }
    }
}

#[test]
fn every_spec_has_nonempty_description_and_query_family() {
    for spec in structural_fact_pattern_specs() {
        assert!(
            !spec.description.trim().is_empty(),
            "{} has an empty description",
            spec.pattern_id
        );
        assert!(
            !spec.query_family.trim().is_empty(),
            "{} has an empty query_family",
            spec.pattern_id
        );
    }
}

#[test]
fn languages_within_a_spec_are_unique() {
    for spec in structural_fact_pattern_specs() {
        let mut seen = HashSet::new();
        for language in spec.languages {
            assert!(
                seen.insert(*language),
                "{} lists language {} more than once",
                spec.pattern_id,
                language
            );
        }
    }
}

#[test]
fn metadata_keys_are_well_formed_and_unique() {
    for spec in structural_fact_pattern_specs() {
        let mut seen = HashSet::new();
        for meta in spec.metadata_keys {
            assert!(
                !meta.key.trim().is_empty(),
                "{} has a metadata key with an empty name",
                spec.pattern_id
            );
            assert!(
                !meta.description.trim().is_empty(),
                "{} metadata key {} has an empty description",
                spec.pattern_id,
                meta.key
            );
            assert!(
                seen.insert(meta.key),
                "{} declares metadata key {} more than once",
                spec.pattern_id,
                meta.key
            );
        }
    }
}

#[test]
fn every_spec_declares_base_metadata_keys() {
    // `pattern_version` and `query_family` are inserted by every collector's
    // `base_metadata`, so they must be declared (as Always) on every spec.
    for spec in structural_fact_pattern_specs() {
        for base in ["pattern_version", "query_family"] {
            let declared = spec
                .metadata_keys
                .iter()
                .find(|meta| meta.key == base)
                .unwrap_or_else(|| {
                    panic!("{} is missing base metadata key {}", spec.pattern_id, base)
                });
            assert_eq!(
                declared.presence,
                KeyPresence::Always,
                "{} declares base key {} as non-Always",
                spec.pattern_id,
                base
            );
        }
    }
}

#[test]
fn framework_key_type_is_string_when_present() {
    for spec in structural_fact_pattern_specs() {
        if let Some(meta) = spec
            .metadata_keys
            .iter()
            .find(|meta| meta.key == "framework")
        {
            assert_eq!(
                meta.value_type,
                MetadataValueType::String,
                "{} declares framework with a non-String type",
                spec.pattern_id
            );
            assert_eq!(
                meta.presence,
                KeyPresence::Always,
                "{} declares framework as non-Always",
                spec.pattern_id
            );
        }
    }
}

/// Primary invariant: the registry's per-language pattern-id set must equal
/// the authoritative union the extractor actually emits for that language
/// (`structural_fact_pattern_ids_for_language`, which unions the built-in
/// patterns and all five base collectors).
///
/// That authority — like the collectors' own `*_pattern_ids_for_language`
/// helpers — is compiled only under the `test-capability-matrix` feature, so
/// this invariant is gated to match. Run it with:
///   `cargo test -p julie-extractors --features test-capability-matrix \
///        structural_fact_registry`.
#[cfg(feature = "test-capability-matrix")]
#[test]
fn registry_pattern_ids_match_emitted_union_per_language() {
    use crate::base::structural_facts::structural_fact_pattern_ids_for_language;
    use crate::qmldir::STRUCTURAL_FACT_PATTERN_IDS;
    use std::collections::BTreeSet;

    // Every language any source emits for. Kept in sync with the collector
    // match arms; unioned with the registry's own languages so a spec that
    // introduces a new language is still checked.
    const KNOWN_LANGUAGES: &[&str] = &[
        // built-in patterns (base/structural_facts.rs)
        "c",
        "cpp",
        "go",
        "javascript",
        "jsx",
        "python",
        "rust",
        "tsx",
        "typescript",
        // code collector
        "dart",
        "elixir",
        "erlang",
        "java",
        "kotlin",
        "lua",
        "php",
        "r",
        "ruby",
        "scala",
        "swift",
        "bash",
        "gdscript",
        "powershell",
        "qml",
        "qmldir",
        "vbnet",
        "zig",
        // data collector
        "markdown",
        "json",
        "toml",
        "yaml",
        "regex",
        "xml", //
        // sql collector
        "sql", //
        // framework + web collectors
        "csharp",
        "html",
        "razor",
        "vue",
        "css",
    ];

    let mut languages: BTreeSet<&str> = KNOWN_LANGUAGES.iter().copied().collect();
    for spec in structural_fact_pattern_specs() {
        languages.extend(spec.languages.iter().copied());
    }

    let mut errors = Vec::new();
    let mut union_from_emission: BTreeSet<String> = BTreeSet::new();
    for language in &languages {
        let registry: BTreeSet<String> = structural_fact_pattern_specs()
            .iter()
            .filter(|spec| spec.languages.contains(language))
            .map(|spec| spec.pattern_id.to_string())
            .collect();
        let mut emitted: BTreeSet<String> = if *language == "qmldir" {
            STRUCTURAL_FACT_PATTERN_IDS
                .iter()
                .map(|pattern_id| (*pattern_id).to_string())
                .collect()
        } else {
            structural_fact_pattern_ids_for_language(language)
                .into_iter()
                .map(str::to_string)
                .collect()
        };
        if structural_fact_pattern_specs()
            .iter()
            .any(|spec| spec.pattern_id == "code.marker.v1" && spec.languages.contains(language))
        {
            emitted.insert("code.marker.v1".to_string());
        }
        union_from_emission.extend(emitted.iter().cloned());
        if registry != emitted {
            let missing: Vec<&String> = emitted.difference(&registry).collect();
            let extra: Vec<&String> = registry.difference(&emitted).collect();
            errors.push(format!(
                "language `{language}` mismatch: missing_from_registry={missing:?} not_emitted={extra:?}"
            ));
        }
    }

    // Global completeness: no registry pattern is dead (never emitted for any
    // known language), and no emitted pattern is unregistered.
    let all_registry: BTreeSet<String> = structural_fact_pattern_specs()
        .iter()
        .map(|spec| spec.pattern_id.to_string())
        .collect();
    for dead in all_registry.difference(&union_from_emission) {
        errors.push(format!(
            "registry pattern `{dead}` is not emitted for any known language"
        ));
    }

    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn qml_domain_fact_ids_have_registered_contracts() {
    let ids = structural_fact_pattern_specs()
        .iter()
        .filter(|spec| spec.languages.contains(&"qml") || spec.languages.contains(&"qmldir"))
        .map(|spec| spec.pattern_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(ids.contains("qml.import_statement.v1"));
    assert!(ids.contains("qml.object_instantiation.v1"));
    assert!(ids.contains("qml.typeinfo_declaration.v1"));
    assert!(ids.contains("qmldir.module.v1"));
    assert!(ids.contains("qmldir.object_type.v1"));
    assert!(ids.contains("qmldir.typeinfo.v1"));
}

#[test]
fn current_sqlite_contract_lists_every_registered_frontend_pattern() {
    let contract_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/sqlite-schema-v4.md");
    let contract = std::fs::read_to_string(&contract_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", contract_path.display()));

    for spec in framework::frontend_specs() {
        assert!(
            contract.contains(spec.pattern_id),
            "{} must be documented in {}",
            spec.pattern_id,
            contract_path.display()
        );
    }
}
