use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::helpers::metadata_str;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/powershell/basic/source.ps1");

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/powershell/basic/source.ps1",
        source,
        Path::new("/repo"),
    )
    .expect("canonical PowerShell extraction should succeed")
}


#[test]
fn powershell_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "powershell.cmdlet_binding_attribute.v1",
        "powershell.param_block.v1",
        "powershell.pipeline_expression.v1",
        "powershell.class_definition.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "missing structural fact pattern `{pattern_id}`"
        );
    }

    let cmdlet_binding = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "powershell.cmdlet_binding_attribute.v1")
        .expect("expected CmdletBinding attribute fact");
    assert_eq!(
        metadata_str(cmdlet_binding, "attribute_name"),
        Some("CmdletBinding")
    );

    let param_block = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "powershell.param_block.v1")
        .expect("expected param block fact");
    assert!(param_block.containing_symbol_id.is_some());

    let pipeline = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "powershell.pipeline_expression.v1")
        .expect("expected pipeline expression fact");
    assert_eq!(metadata_str(pipeline, "pipeline_marker"), Some("|"));

    let class_def = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "powershell.class_definition.v1")
        .expect("expected class definition fact");
    assert_eq!(metadata_str(class_def, "class_name"), Some("Worker"));
}

#[test]
fn powershell_typed_parameter_attributes_are_not_cmdlet_binding() {
    let source = r#"
function Invoke-Thing {
    param([int]$Count)
}
"#;
    let results = extract(source);
    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.pattern_id != "powershell.cmdlet_binding_attribute.v1"),
        "[int] parameter attributes must not emit CmdletBinding structural facts"
    );
}
