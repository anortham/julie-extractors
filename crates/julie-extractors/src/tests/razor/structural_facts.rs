use std::path::Path;

use serde_json::Value;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.razor", source, Path::new("/repo"))
        .expect("canonical Razor extraction should succeed")
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
fn explicit_razor_expression_emits_template_expression_fact() {
    let results = extract("<p>@(1 + 2)</p>");
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert!(
        facts_with_pattern(&results, "razor.template_expression.v1")
            .iter()
            .any(|fact| {
                fact.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("expression"))
                    .is_some()
            })
    );
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
    assert_eq!(
        metadata_str(page, "normalized_route_template"),
        Some("/worker")
    );
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
    assert_eq!(
        metadata_str(page, "normalized_route_template"),
        Some("/todos/:id")
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
        metadata_str(optional_page, "route_template"),
        Some("/orders/{orderId?}")
    );
    assert_eq!(
        metadata_bool_field(&optional_params[0], "optional"),
        Some(true)
    );

    let catch_all = extract(r#"@page "/files/{*path}""#);
    let catch_all_page = facts_with_parameter(&catch_all, "path").expect("catch-all route");
    let catch_all_params = route_parameters(catch_all_page);
    assert_eq!(
        metadata_str(catch_all_page, "route_template"),
        Some("/files/{*path}")
    );
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

#[test]
fn razor_navigation_calls_emit_literal_route_references() {
    let source = r#"@inject NavigationManager Navigation

@code {
    void OpenOrders() => Navigation.NavigateTo("/orders/{id?}");
    void OpenLogin() => Navigation.NavigateToLogin("/authentication/login", new());
}
"#;

    let results = extract(source);
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");

    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(facts.iter().any(|fact| {
        metadata_str(fact, "target_path") == Some("/orders/{id?}")
            && metadata_str(fact, "source_kind") == Some("navigate_to")
    }));
    assert!(facts.iter().any(|fact| {
        metadata_str(fact, "target_path") == Some("/authentication/login")
            && metadata_str(fact, "source_kind") == Some("navigate_to_login")
    }));
    for fact in facts {
        assert_eq!(metadata_str(fact, "route_source"), Some("string_literal"));
        assert_eq!(metadata_str(fact, "framework"), Some("blazor"));
    }
}

#[test]
fn razor_internal_href_emits_one_raw_route_reference() {
    let source = r##"<nav>
    <a href="/orders/{id?}">Orders</a>
    <a href="https://example.com/orders">External</a>
    <a href="http://example.com/orders">External HTTP</a>
    <a href="#details">Details</a>
    <a href="@OrderUrl">Dynamic</a>
</nav>"##;

    let results = extract(source);
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/orders/{id?}"));
    assert_eq!(metadata_str(facts[0], "source_kind"), Some("href"));
    assert_eq!(
        metadata_str(facts[0], "route_source"),
        Some("string_literal")
    );
    assert_eq!(metadata_str(facts[0], "framework"), Some("blazor"));
}

#[test]
fn razor_href_scanning_accepts_internal_unquoted_values_after_unquoted_attributes() {
    let source = r##"<nav>
    <a class=button href=/orders/internal>Orders</a>
    <a class=button href=https://example.com/orders>External</a>
    <a class=button href=#details>Details</a>
</nav>"##;

    let results = extract(source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "expected clean Razor parse: {:#?}",
        results.parse_diagnostics
    );
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "target_path"),
        Some("/orders/internal")
    );
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "/orders/internal"
    );
}

#[test]
fn razor_unquoted_dynamic_href_does_not_emit_route_reference() {
    let results = extract("<a class=button href=@OrderUrl>Dynamic</a>");

    assert!(facts_with_pattern(&results, "razor.route_reference.v1").is_empty());
}

#[test]
fn razor_malformed_attribute_recovers_following_route_fact() {
    let source = "<div class=\"broken></div>\n<a href=/orders/recovered>Recovered</a>";
    let results = extract(source);

    assert!(
        !results.parse_diagnostics.is_empty(),
        "malformed input must not be labeled clean"
    );
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "target_path"),
        Some("/orders/recovered")
    );
    assert_eq!(metadata_str(facts[0], "source_kind"), Some("href"));
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "/orders/recovered"
    );
}

#[test]
fn razor_href_route_fact_span_is_the_deeply_nested_non_self_closing_value() {
    let source = r#"<main>
    <section>
        <article>
            <div>
                <a class="nav" href="/orders/deep">Orders <span>now</span></a>
            </div>
        </article>
    </section>
</main>"#;

    let results = extract(source);
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");

    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(
        &source[fact.start_byte as usize..fact.end_byte as usize],
        "/orders/deep"
    );
    assert_eq!((fact.start_line, fact.start_column), (5, 37));
    assert_eq!((fact.end_line, fact.end_column), (5, 49));
}

#[test]
fn razor_navigation_skips_dynamic_arguments_and_unproven_receivers() {
    let source = r#"@inject NavigationManager Navigation

@code {
    string OrderUrl = "/orders";
    void OpenProven() => Navigation.NavigateTo("/orders");
    void SkipDynamic() => Navigation.NavigateTo(OrderUrl);
    void SkipInterpolated() => Navigation.NavigateTo($"/orders/{OrderId}");
    void SkipUnproven() => router.NavigateTo("/admin");
    void SkipShadowed(Router Navigation) => Navigation.NavigateTo("/shadowed");
}
"#;

    let results = extract(source);
    let facts = facts_with_pattern(&results, "razor.route_reference.v1");

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/orders"));
}

#[test]
fn blazor_event_and_bind_directives_do_not_emit_alpine_facts() {
    let source = r#"
<button @onclick="Save" @bind="Name" @onchange="Changed" @ref="buttonRef">
    Save
</button>

@code {
    string Name { get; set; } = "";
    void Save() {}
    void Changed() {}
}
"#;
    let results = extract(source);
    let alpine = facts_with_pattern(&results, "alpine.directive.v1");
    assert!(
        alpine.is_empty(),
        "Blazor @onclick/@bind/@onchange/@ref attributes must not be classified as Alpine directives: {alpine:#?}"
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
