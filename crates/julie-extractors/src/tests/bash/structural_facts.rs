use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/bash/basic/source.sh");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/bash/basic/source.sh",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Bash extraction should succeed")
}


#[test]
fn bash_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "bash.shebang.v1",
        "bash.command_substitution.v1",
        "bash.arithmetic_expansion.v1",
        "bash.export_declaration.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let shebang = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "bash.shebang.v1")
        .expect("expected shebang fact");
    assert!(shebang.start_line == 1);

    let export = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "bash.export_declaration.v1")
        .expect("expected export declaration fact");
    assert_eq!(metadata_str(export, "variable_name"), Some("APP_ENV"));

    let command_subs = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "bash.command_substitution.v1")
        .count();
    assert_eq!(
        command_subs, 1,
        "fixture contains one command substitution in the for-loop"
    );

    let arithmetic = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "bash.arithmetic_expansion.v1")
        .count();
    assert!(
        arithmetic >= 2,
        "fixture contains helper and loop arithmetic expansions"
    );
}

#[test]
fn bash_command_substitution_does_not_match_plain_commands() {
    let source = r#"#!/bin/bash

helper() {
    echo hello
}
"#;
    let results = extract(source);
    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.pattern_id != "bash.command_substitution.v1"),
        "plain command invocations must not emit bash.command_substitution.v1"
    );
}
