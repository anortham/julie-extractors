//! Symbols-level extraction gates the reference and text/facts families
//! uniformly across languages. `strip_to_symbols_level` is the single
//! authority on the gated set; these tests pin both directions — what a
//! symbols-level extraction may never carry, and what it must keep identical
//! to a full-level extraction of the same source.

use std::path::Path;

use crate::ExtractionLevel;
use crate::pipeline::{extract_canonical, extract_canonical_at};

fn extract_at(level: ExtractionLevel, file_path: &str, source: &str) -> crate::ExtractionResults {
    extract_canonical_at(file_path, source, Path::new("/repo"), level)
        .expect("canonical extraction should succeed")
}

struct LevelFixture {
    language: &'static str,
    file_path: &'static str,
    source: &'static str,
}

/// Rust exercises the macro registry path; sql, markdown, and regex record
/// literals OUTSIDE their identifier walks, so they prove the strip keeps the
/// level uniform instead of leaving a silent three-language literal subset.
const FIXTURES: &[LevelFixture] = &[
    LevelFixture {
        language: "rust",
        file_path: "src/lib.rs",
        source: "// comment\n/// docs\npub fn alpha() { helper(\"hello\"); }\npub fn helper(v: &str) -> &str { v }\n",
    },
    LevelFixture {
        language: "sql",
        file_path: "db/schema.sql",
        source: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT DEFAULT 'anon');\nINSERT INTO users (name) VALUES ('alpha');\n",
    },
    LevelFixture {
        language: "markdown",
        file_path: "docs/readme.md",
        source: "# Title\n\nSome prose with a [link](https://example.com).\n\n```rust\nfn embedded() {}\n```\n",
    },
    LevelFixture {
        language: "regex",
        file_path: "patterns/route.regex",
        source: "^/api/(?<version>v[0-9]+)/users/(?<id>[0-9]+)$",
    },
];

#[test]
fn symbols_level_never_carries_gated_families_for_any_fixture_language() {
    for fixture in FIXTURES {
        let results = extract_at(ExtractionLevel::Symbols, fixture.file_path, fixture.source);
        assert!(
            results.identifiers.is_empty(),
            "{}: identifiers must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.literals.is_empty(),
            "{}: literals must be empty at symbols level (even for languages that record them outside the identifier walk)",
            fixture.language
        );
        assert!(
            results.type_argument_usages.is_empty(),
            "{}: type argument usages must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.source_regions.is_empty(),
            "{}: source regions must be empty at symbols level",
            fixture.language
        );
        assert!(
            results.structural_facts.is_empty(),
            "{}: structural facts must be empty at symbols level",
            fixture.language
        );
    }
}

#[test]
fn symbols_level_keeps_symbols_relationships_and_complexity_identical_to_full() {
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        let symbols = extract_at(ExtractionLevel::Symbols, fixture.file_path, fixture.source);
        assert_eq!(
            full.symbols.len(),
            symbols.symbols.len(),
            "{}: symbol extraction must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.relationships.len(),
            symbols.relationships.len(),
            "{}: relationships must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.pending_relationships.len(),
            symbols.pending_relationships.len(),
            "{}: pending relationships must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.complexity_metrics.len(),
            symbols.complexity_metrics.len(),
            "{}: complexity metrics must be identical across levels",
            fixture.language
        );
        assert_eq!(
            full.parse_diagnostics.len(),
            symbols.parse_diagnostics.len(),
            "{}: parse diagnostics must be identical across levels",
            fixture.language
        );
    }
}

#[test]
fn full_level_fixtures_actually_exercise_the_gated_families() {
    let mut saw_identifiers = false;
    let mut saw_literals = false;
    let mut saw_regions = false;
    for fixture in FIXTURES {
        let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
        saw_identifiers |= !full.identifiers.is_empty();
        saw_literals |= !full.literals.is_empty();
        saw_regions |= !full.source_regions.is_empty();
    }
    assert!(
        saw_identifiers && saw_literals && saw_regions,
        "the fixtures must produce identifiers/literals/regions at full level, \
         or the symbols-level emptiness assertions are vacuous \
         (identifiers={saw_identifiers}, literals={saw_literals}, regions={saw_regions})"
    );
}

#[test]
fn extract_canonical_defaults_to_full_level() {
    let fixture = &FIXTURES[0];
    let default = extract_canonical(fixture.file_path, fixture.source, Path::new("/repo"))
        .expect("canonical extraction should succeed");
    let full = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
    assert_eq!(default.identifiers.len(), full.identifiers.len());
    assert_eq!(default.source_regions.len(), full.source_regions.len());
    assert!(
        !default.identifiers.is_empty(),
        "the default path must keep extracting the reference layer"
    );
}

#[test]
fn strip_to_symbols_level_is_the_single_authority_on_the_gated_set() {
    let fixture = &FIXTURES[0];
    let mut results = extract_at(ExtractionLevel::Full, fixture.file_path, fixture.source);
    let kept_symbols = results.symbols.len();
    let kept_relationships = results.relationships.len();
    let kept_complexity = results.complexity_metrics.len();
    results.strip_to_symbols_level();
    assert!(results.identifiers.is_empty());
    assert!(results.literals.is_empty());
    assert!(results.type_argument_usages.is_empty());
    assert!(results.source_regions.is_empty());
    assert!(results.structural_facts.is_empty());
    assert_eq!(results.symbols.len(), kept_symbols);
    assert_eq!(results.relationships.len(), kept_relationships);
    assert_eq!(results.complexity_metrics.len(), kept_complexity);
}
