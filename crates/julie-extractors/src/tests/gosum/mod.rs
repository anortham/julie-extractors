use std::path::Path;

use crate::base::{ExtractionResults, StructuralFact};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const PATTERN_ID: &str = "gosum.checksum.v1";

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("/repo/app/go.sum", source, Path::new("/repo")).unwrap()
}

fn checksums(results: &ExtractionResults) -> Vec<&StructuralFact> {
    let mut facts = facts_with_pattern(results, PATTERN_ID);
    facts.sort_by_key(|fact| fact.start_byte);
    facts
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata.as_ref()?.get(key)?.as_bool()
}

fn summary(fact: &StructuralFact) -> String {
    format!(
        "{} {} go_mod={} incompatible={} pseudo={} timestamp={} revision={}",
        metadata_str(fact, "module_path").unwrap(),
        metadata_str(fact, "version").unwrap(),
        metadata_bool(fact, "go_mod").unwrap(),
        metadata_bool(fact, "incompatible").unwrap(),
        metadata_bool(fact, "pseudo_version").unwrap(),
        metadata_str(fact, "timestamp").unwrap_or("-"),
        metadata_str(fact, "revision").unwrap_or("-"),
    )
}

#[test]
fn go_sum_basename_selects_gosum_in_any_case() {
    for path in ["go.sum", "sub/go.sum", "sub/GO.SUM"] {
        assert_eq!(
            crate::language::detect_language_for_path(Path::new(path), ""),
            Some("gosum"),
            "{path}"
        );
    }
    for path in ["go.mod", "go.work.sum", "go.sum.orig", "x.sum"] {
        assert_ne!(
            crate::language::detect_language_for_path(Path::new(path), ""),
            Some("gosum"),
            "{path}"
        );
    }
}

#[test]
fn each_line_is_a_checksum_fact_with_its_hash_and_line_span() {
    let source = "example.com/a v1.2.3 h1:vj9j/u1bqnvCEfJOwUhtlOARqs3+rkHYY13jYWTU97c=\nexample.com/a v1.2.3/go.mod h1:J7Y8YcW2NihsgmVo/mv3lAwl/skON4iLHjSsI+c5H38=\n";
    let results = extract(source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let facts = checksums(&results);
    assert_eq!(facts.len(), 2);
    let module = facts[0];
    assert_eq!(metadata_str(module, "hash_algorithm"), Some("h1"));
    assert_eq!(
        metadata_str(module, "hash"),
        Some("vj9j/u1bqnvCEfJOwUhtlOARqs3+rkHYY13jYWTU97c=")
    );
    assert_eq!(metadata_str(module, "query_family"), Some("dependencies"));
    assert_eq!(
        &source[module.start_byte as usize..module.end_byte as usize],
        "example.com/a v1.2.3 h1:vj9j/u1bqnvCEfJOwUhtlOARqs3+rkHYY13jYWTU97c="
    );
    assert_eq!((facts[1].start_line, facts[1].end_line), (2, 2));
    assert_eq!(
        metadata_str(facts[1], "hash"),
        Some("J7Y8YcW2NihsgmVo/mv3lAwl/skON4iLHjSsI+c5H38=")
    );
}

#[test]
fn versions_report_go_mod_incompatible_and_pseudo_version_shapes() {
    let source = "\
example.com/a v0.0.0-20161208181325-20d25e280405 h1:x=
example.com/b v1.4.0-rc.1.0.20200221234624-67d41d38c208/go.mod h1:x=
example.com/c v0.4.1-0.20220921163831-55ab3332a786 h1:x=
example.com/d v2.0.0-20190101000000-abcdef123456+incompatible h1:x=
example.com/e v2.1.0+incompatible/go.mod h1:x=
example.com/f v1.4.0-rc.1 h1:x=
example.com/g v0.1.1-deprecated h1:x=
example.com/h v1.2.3-20200101000000-abcdef123456 h1:x=
";
    let results = extract(source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert_eq!(
        checksums(&results)
            .into_iter()
            .map(summary)
            .collect::<Vec<_>>(),
        vec![
            "example.com/a v0.0.0-20161208181325-20d25e280405 go_mod=false incompatible=false pseudo=true timestamp=20161208181325 revision=20d25e280405",
            "example.com/b v1.4.0-rc.1.0.20200221234624-67d41d38c208 go_mod=true incompatible=false pseudo=true timestamp=20200221234624 revision=67d41d38c208",
            "example.com/c v0.4.1-0.20220921163831-55ab3332a786 go_mod=false incompatible=false pseudo=true timestamp=20220921163831 revision=55ab3332a786",
            "example.com/d v2.0.0-20190101000000-abcdef123456+incompatible go_mod=false incompatible=true pseudo=true timestamp=20190101000000 revision=abcdef123456",
            "example.com/e v2.1.0+incompatible go_mod=true incompatible=true pseudo=false timestamp=- revision=-",
            "example.com/f v1.4.0-rc.1 go_mod=false incompatible=false pseudo=false timestamp=- revision=-",
            "example.com/g v0.1.1-deprecated go_mod=false incompatible=false pseudo=false timestamp=- revision=-",
            "example.com/h v1.2.3-20200101000000-abcdef123456 go_mod=false incompatible=false pseudo=false timestamp=- revision=-",
        ]
    );
}

#[test]
fn a_go_sum_file_publishes_only_structural_facts() {
    let results = extract("example.com/a v1.0.0 h1:x=\nexample.com/a v1.0.0/go.mod h1:y=\n");

    assert_eq!(checksums(&results).len(), 2);
    assert_eq!(results.structural_facts.len(), 2);
    assert!(results.symbols.is_empty());
    assert!(results.relationships.is_empty());
    assert!(results.pending_relationships.is_empty());
    assert!(results.structured_pending_relationships.is_empty());
    assert!(results.identifiers.is_empty());
    assert!(results.literals.is_empty());
    assert!(results.source_regions.is_empty());
    assert!(results.complexity_metrics.is_empty());
}

#[test]
fn empty_blank_and_crlf_files_parse_without_diagnostics() {
    for (source, expected) in [
        ("", 0),
        ("\n\n", 0),
        (
            "example.com/a v1.0.0 h1:x=\r\n\r\nexample.com/b v1.0.0 h1:y=\r\n",
            2,
        ),
        ("example.com/a v1.0.0 h1:x=", 1),
    ] {
        let results = extract(source);

        assert!(
            results.parse_diagnostics.is_empty(),
            "{source:?}: {:#?}",
            results.parse_diagnostics
        );
        assert_eq!(checksums(&results).len(), expected, "{source:?}");
    }
    let crlf = extract("example.com/a v1.0.0 h1:x=\r\n");
    assert_eq!(metadata_str(checksums(&crlf)[0], "hash"), Some("x="));
}

#[test]
fn a_malformed_line_reports_a_diagnostic_and_publishes_no_row_for_it() {
    let results = extract(
        "example.com/a v1.0.0 h1:x=\nexample.com/b v1.0.0\nexample.com/c v1.0.0/go.mod h1:y=\n\nexample.com/d v1.0.0 h1:z=\n",
    );

    assert!(!results.parse_diagnostics.is_empty());
    let modules: Vec<&str> = checksums(&results)
        .into_iter()
        .map(|fact| metadata_str(fact, "module_path").unwrap())
        .collect();
    assert!(modules.contains(&"example.com/a"), "{modules:?}");
    assert!(modules.contains(&"example.com/d"), "{modules:?}");
    assert!(!modules.contains(&"example.com/b"), "{modules:?}");
}
