use std::path::Path;

use serde_json::Value;

use crate::base::{StructuralFact, SymbolKind};

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical Razor extraction should succeed")
}

fn component_facts(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "blazor.component_reference.v1")
        .collect()
}

fn metadata<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a Value> {
    fact.metadata.as_ref()?.get(key)
}

#[test]
fn cross_file_component_tag_emits_unresolved_reference_context() {
    let source = r#"@namespace Sample.Pages
@using Sample.Components

<SharedWidget />
"#;
    let results = extract("Pages/PageA.razor", source);

    let facts = component_facts(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(
        metadata(fact, "tag").and_then(Value::as_str),
        Some("SharedWidget")
    );
    assert_eq!(
        metadata(fact, "containing_component").and_then(Value::as_str),
        Some("PageA")
    );
    assert_eq!(
        metadata(fact, "namespace_context").and_then(Value::as_array),
        Some(&vec![
            Value::String("Sample.Pages".to_string()),
            Value::String("Sample.Components".to_string()),
        ])
    );
    assert_eq!(
        metadata(fact, "generic_arguments").and_then(Value::as_array),
        Some(&vec![])
    );
    assert!(metadata(fact, "external").is_none());
    assert!(fact.start_byte < fact.end_byte);
    assert_eq!(
        &source[fact.start_byte as usize..fact.end_byte as usize],
        "<SharedWidget />"
    );
}

#[test]
fn fluent_component_facts_keep_local_context_and_generic_arguments() {
    let results = extract(
        "Pages/Orders.razor",
        r#"@namespace Sample.Pages
@using Microsoft.FluentUI.AspNetCore.Components
@using Sample.Models

<FluentButton>Save</FluentButton>
<FluentDataGrid TGridItem="OrderRow" Items="@orders" />
"#,
    );

    let facts = component_facts(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    for fact in &facts {
        assert_eq!(
            metadata(fact, "containing_component").and_then(Value::as_str),
            Some("Orders")
        );
        assert_eq!(
            metadata(fact, "namespace_context").and_then(Value::as_array),
            Some(&vec![
                Value::String("Sample.Pages".to_string()),
                Value::String("Microsoft.FluentUI.AspNetCore.Components".to_string()),
                Value::String("Sample.Models".to_string()),
            ])
        );
        assert!(metadata(fact, "external").is_none());
    }

    let grid = facts
        .iter()
        .find(|fact| metadata(fact, "tag").and_then(Value::as_str) == Some("FluentDataGrid"))
        .expect("expected FluentDataGrid reference");
    assert_eq!(
        metadata(grid, "generic_arguments").and_then(Value::as_array),
        Some(&vec![serde_json::json!({
            "name": "TGridItem",
            "value": "OrderRow"
        })])
    );
}

#[test]
fn lowercase_html_tags_do_not_emit_component_references() {
    let results = extract(
        "Pages/Index.razor",
        r#"<main>
    <section><button type="button">Save</button></section>
    <My-Widget />
    <MY_WIDGET />
</main>"#,
    );

    assert!(component_facts(&results).is_empty());
}

#[test]
fn dynamic_generic_component_arguments_stay_out_of_reference_metadata() {
    let results = extract(
        "Pages/Dynamic.razor",
        r#"<DynamicGrid TGridItem="@rowType" />
<ExplicitGrid TGridItem="@(typeof(OrderRow))" />
<CallGrid TGridItem="ResolveType()" />"#,
    );

    let facts = component_facts(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");
    for fact in facts {
        assert_eq!(
            metadata(fact, "generic_arguments").and_then(Value::as_array),
            Some(&vec![]),
            "dynamic type expressions are not static generic arguments: {fact:#?}"
        );
    }
}

#[test]
fn razor_infrastructure_files_do_not_emit_component_symbols() {
    for file_path in [
        "_Imports.razor",
        "Views/_ViewImports.razor",
        "Views/_ViewImports.cshtml",
    ] {
        let results = extract(file_path, "@using Sample.Components\n");
        assert!(
            results.symbols.iter().all(|symbol| {
                symbol.kind != SymbolKind::Class
                    || symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("type"))
                        .and_then(Value::as_str)
                        != Some("razor-component")
            }),
            "{file_path} must not define a synthetic component: {:#?}",
            results.symbols
        );
    }
}

#[test]
fn app_razor_still_emits_component_symbol() {
    let results = extract(
        "App.razor",
        "<Router AppAssembly=\"@typeof(App).Assembly\" />\n",
    );

    assert!(results.symbols.iter().any(|symbol| {
        symbol.name == "App"
            && symbol.kind == SymbolKind::Class
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("type"))
                .and_then(Value::as_str)
                == Some("razor-component")
    }));
}
