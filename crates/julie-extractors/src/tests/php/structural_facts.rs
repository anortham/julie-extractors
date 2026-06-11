use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/php/basic/source.php");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/php/basic/source.php",
        source,
        Path::new("/repo"),
    )
    .expect("canonical PHP extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn php_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "php.attribute.v1",
        "php.namespace_definition.v1",
        "php.namespace_use_declaration.v1",
        "php.trait_use_declaration.v1",
        "php.anonymous_function.v1",
        "php.match_expression.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let namespace = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "php.namespace_definition.v1")
        .expect("expected namespace definition fact");
    assert_eq!(metadata_str(namespace, "namespace_name"), Some("Fixture"));
    assert!(namespace.containing_symbol_id.is_some());

    let import = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "php.namespace_use_declaration.v1"
                && metadata_str(fact, "import_target")
                    == Some("Symfony\\Component\\HttpFoundation\\Response")
        })
        .expect("expected namespace use declaration fact");
    assert_eq!(metadata_str(import, "import_alias"), Some("HttpResponse"));
    assert_ne!(
        metadata_str(import, "import_target"),
        metadata_str(import, "import_alias"),
        "import target must not be confused with local alias"
    );

    let trait_use = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "php.trait_use_declaration.v1")
        .expect("expected trait use fact");
    assert_eq!(metadata_str(trait_use, "trait_name"), Some("Timestampable"));
}
