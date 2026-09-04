use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str = include_str!("../../../../../fixtures/extraction/r/basic/source.r");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/r/basic/source.r",
        source,
        Path::new("/repo"),
    )
    .expect("canonical R extraction should succeed")
}


#[test]
fn r_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "r.library_call.v1",
        "r.pipe_expression.v1",
        "r.formula_expression.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let library = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "r.library_call.v1")
        .expect("expected library call fact");
    assert_eq!(metadata_str(library, "load_kind"), Some("library"));
    assert_eq!(metadata_str(library, "package_name"), Some("dplyr"));

    let pipe_facts = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "r.pipe_expression.v1")
        .collect::<Vec<_>>();
    assert_eq!(
        pipe_facts.len(),
        1,
        "fixture contains one pipe operator; ancestor binary operators must not match"
    );
    assert!(pipe_facts[0].containing_symbol_id.is_some());

    let formula = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "r.formula_expression.v1")
        .expect("expected formula expression fact");
    assert_eq!(metadata_str(formula, "formula_text"), Some("total ~ count"));
    assert_ne!(
        metadata_str(library, "package_name"),
        metadata_str(formula, "formula_text"),
        "library package name must not be confused with formula text"
    );
}
