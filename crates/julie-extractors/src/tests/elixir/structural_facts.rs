use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/elixir/basic/source.ex");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/elixir/basic/source.ex",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Elixir extraction should succeed")
}

#[test]
fn elixir_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "elixir.defmodule_call.v1",
        "elixir.module_attribute.v1",
        "elixir.directive_call.v1",
        "elixir.pipeline_operator.v1",
        "elixir.with_expression.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let defmodule = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "elixir.defmodule_call.v1")
        .expect("expected defmodule call fact");
    assert_eq!(
        metadata_str(defmodule, "module_name"),
        Some("Fixture.Worker")
    );

    let spec = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "elixir.module_attribute.v1"
                && metadata_str(fact, "attribute_name") == Some("spec")
        })
        .expect("expected @spec module attribute fact");
    assert_eq!(metadata_str(spec, "query_family"), Some("metadata"));

    let import = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "elixir.directive_call.v1"
                && metadata_str(fact, "directive_kind") == Some("import")
        })
        .expect("expected import directive fact");
    assert_eq!(metadata_str(import, "directive_target"), Some("Kernel"));

    let alias = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "elixir.directive_call.v1"
                && metadata_str(fact, "directive_kind") == Some("alias")
        })
        .expect("expected alias directive fact");
    assert_eq!(
        metadata_str(alias, "directive_target"),
        Some("Fixture.Helper")
    );
    assert_ne!(
        metadata_str(import, "directive_target"),
        metadata_str(alias, "directive_target"),
        "import target must not be confused with alias target"
    );

    let pipeline = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "elixir.pipeline_operator.v1")
        .expect("expected pipeline operator fact");
    assert_eq!(metadata_str(pipeline, "query_family"), Some("pipeline"));
}
