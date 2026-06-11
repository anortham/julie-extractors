use std::path::Path;

use serde_json::Value;

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

fn metadata_number(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

fn metadata_object_field<'a>(parameter: &'a Value, key: &str) -> Option<&'a str> {
    parameter.as_object()?.get(key)?.as_str()
}

fn metadata_bool_field(parameter: &Value, key: &str) -> Option<bool> {
    parameter.as_object()?.get(key)?.as_bool()
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
    assert_eq!(metadata_str(page, "route_template"), Some("/worker"));
    assert_eq!(metadata_number(page, "route_parameter_count"), Some(0));
    assert_eq!(metadata_bool(page, "has_route_constraints"), Some(false));
    assert_eq!(
        page.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("route_parameters"))
            .and_then(|value| value.as_array())
            .map(|array| array.len()),
        Some(0)
    );

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

#[test]
fn razor_page_directive_facts_parse_route_parameters() {
    let source = r#"@page "/todos/{id:int}"

<h1>Todo</h1>
"#;

    let results = extract(source);
    let page = facts_with_pattern(&results, "razor.page_directive.v1")
        .into_iter()
        .next()
        .expect("expected constrained page directive fact");
    assert_eq!(
        metadata_str(page, "route_template"),
        Some("/todos/{id:int}")
    );
    assert_eq!(metadata_number(page, "route_parameter_count"), Some(1));
    assert_eq!(metadata_bool(page, "has_route_constraints"), Some(true));

    let parameters = page
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("route_parameters"))
        .and_then(|value| value.as_array())
        .expect("expected route_parameters array");
    assert_eq!(parameters.len(), 1);
    assert_eq!(metadata_object_field(&parameters[0], "name"), Some("id"));
    assert_eq!(
        metadata_object_field(&parameters[0], "constraint"),
        Some("int")
    );
    assert_eq!(metadata_bool_field(&parameters[0], "optional"), Some(false));
    assert_eq!(
        metadata_bool_field(&parameters[0], "catch_all"),
        Some(false)
    );
}

#[test]
fn razor_page_directive_facts_parse_optional_and_catch_all_parameters() {
    let optional = extract(r#"@page "/orders/{orderId?}""#);
    let optional_page = facts_with_parameter(&optional, "orderId").expect("optional route");
    assert_eq!(
        metadata_bool(optional_page, "has_route_constraints"),
        Some(false)
    );
    let optional_params = route_parameters(optional_page);
    assert_eq!(
        metadata_bool_field(&optional_params[0], "optional"),
        Some(true)
    );

    let catch_all = extract(r#"@page "/files/{*path}""#);
    let catch_all_page = facts_with_parameter(&catch_all, "path").expect("catch-all route");
    let catch_all_params = route_parameters(catch_all_page);
    assert_eq!(
        metadata_bool_field(&catch_all_params[0], "catch_all"),
        Some(true)
    );

    let multi = extract(r#"@page "/archive/{year:int}-{month:int}""#);
    let multi_page = facts_with_pattern(&multi, "razor.page_directive.v1")
        .into_iter()
        .next()
        .expect("multi-parameter route");
    let multi_params = route_parameters(multi_page);
    assert_eq!(multi_params.len(), 2);
    assert_eq!(
        metadata_object_field(&multi_params[0], "name"),
        Some("year")
    );
    assert_eq!(
        metadata_object_field(&multi_params[0], "constraint"),
        Some("int")
    );
    assert_eq!(
        metadata_object_field(&multi_params[1], "name"),
        Some("month")
    );
    assert_eq!(
        metadata_object_field(&multi_params[1], "constraint"),
        Some("int")
    );
}

fn facts_with_parameter<'a>(
    results: &'a crate::ExtractionResults,
    name: &str,
) -> Option<&'a StructuralFact> {
    facts_with_pattern(results, "razor.page_directive.v1")
        .into_iter()
        .find(|fact| {
            route_parameters(fact)
                .first()
                .and_then(|parameter| metadata_object_field(parameter, "name"))
                == Some(name)
        })
}

fn route_parameters(fact: &StructuralFact) -> &[Value] {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("route_parameters"))
        .and_then(|value| value.as_array())
        .map(|array| array.as_slice())
        .unwrap_or(&[])
}
