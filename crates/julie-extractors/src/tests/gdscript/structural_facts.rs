use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/gdscript/basic/source.gd");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/gdscript/basic/source.gd",
        source,
        Path::new("/repo"),
    )
    .expect("canonical GDScript extraction should succeed")
}


#[test]
fn gdscript_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "gdscript.class_name.v1",
        "gdscript.extends_declaration.v1",
        "gdscript.signal_declaration.v1",
        "gdscript.export_annotation.v1",
        "gdscript.match_statement.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let class_name = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "gdscript.class_name.v1")
        .expect("expected class_name fact");
    assert_eq!(metadata_str(class_name, "class_name"), Some("Worker"));

    let extends = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "gdscript.extends_declaration.v1")
        .expect("expected extends declaration fact");
    assert_eq!(metadata_str(extends, "base_type"), Some("Node"));

    let signal = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "gdscript.signal_declaration.v1")
        .expect("expected signal declaration fact");
    assert_eq!(metadata_str(signal, "signal_name"), Some("activated"));

    let export = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "gdscript.export_annotation.v1")
        .expect("expected export annotation fact");
    assert_eq!(metadata_str(export, "annotation_name"), Some("export"));
    assert_eq!(metadata_str(export, "exported_variable"), Some("id"));

    let match_stmt = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "gdscript.match_statement.v1")
        .expect("expected match statement fact");
    assert!(match_stmt.containing_symbol_id.is_some());
}

#[test]
fn gdscript_match_statement_emits_single_fact_per_match() {
    let source = r#"
extends Node

func pick(value: int) -> int:
    match value % 2:
        0:
            return 0
        _:
            return 1
    return 0
"#;
    let results = extract(source);
    let matches = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "gdscript.match_statement.v1")
        .count();
    assert_eq!(
        matches, 1,
        "nested match arms must not duplicate match facts"
    );
}
