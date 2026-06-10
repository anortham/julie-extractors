use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.razor", source, Path::new("/repo"))
        .expect("canonical Razor extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

#[test]
fn razor_emits_page_code_block_and_template_expression_facts() {
    let source = r#"@page "/worker"

<h1>@Format(Title)</h1>

@code {
    public string Title { get; set; } = "Worker";
}
"#;

    let results = extract(source);

    let page = facts_with_pattern(&results, "razor.page_directive.v1")
        .into_iter()
        .next()
        .expect("expected page directive fact");
    assert_eq!(metadata_str(page, "route"), Some("/worker"));
    assert_eq!(metadata_str(page, "directive"), Some("page"));

    let code_block = facts_with_pattern(&results, "razor.code_block.v1")
        .into_iter()
        .next()
        .expect("expected code block fact");
    assert_eq!(metadata_str(code_block, "block_type"), Some("code"));

    let expression = facts_with_pattern(&results, "razor.template_expression.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "expression") == Some("Format(Title)"))
        .expect("expected template expression fact");
    assert!(metadata_bool(expression, "implicit").is_some());

    assert!(page.start_byte < page.end_byte);
    assert!(code_block.start_byte < code_block.end_byte);
    assert!(expression.start_byte < expression.end_byte);
}
