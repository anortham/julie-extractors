use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/ruby/basic/source.rb");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/ruby/basic/source.rb",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Ruby extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn ruby_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "ruby.require_call.v1",
        "ruby.mixin_call.v1",
        "ruby.block.v1",
        "ruby.rescue_clause.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let require = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "ruby.require_call.v1"
                && metadata_str(fact, "require_kind") == Some("require")
        })
        .expect("expected require call fact");
    assert_eq!(metadata_str(require, "required_path"), Some("json"));

    let require_relative = results
        .structural_facts
        .iter()
        .find(|fact| {
            fact.pattern_id == "ruby.require_call.v1"
                && metadata_str(fact, "require_kind") == Some("require_relative")
        })
        .expect("expected require_relative call fact");
    assert_eq!(
        metadata_str(require_relative, "required_path"),
        Some("./helper")
    );
    assert_ne!(
        metadata_str(require, "require_kind"),
        metadata_str(require_relative, "require_kind"),
        "require and require_relative must remain distinct"
    );

    let mixin = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "ruby.mixin_call.v1")
        .expect("expected mixin call fact");
    assert_eq!(metadata_str(mixin, "mixin_kind"), Some("include"));
    assert_eq!(metadata_str(mixin, "mixin_target"), Some("Enumerable"));

    let rescue = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "ruby.rescue_clause.v1")
        .expect("expected rescue clause fact");
    assert_eq!(
        metadata_str(rescue, "exception_type"),
        Some("ZeroDivisionError")
    );
}

#[test]
fn ruby_require_path_normalizes_single_and_double_quotes() {
    let source = r#"
require 'single_quoted'
require_relative "./double_quoted"
"#;
    let results = extract(source);
    let paths = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "ruby.require_call.v1")
        .map(|fact| metadata_str(fact, "required_path").expect("required_path metadata"))
        .collect::<Vec<_>>();
    assert_eq!(paths, ["single_quoted", "./double_quoted"]);
}
