use std::path::Path;

use serde_json::Value;

use crate::base::{IdentifierKind, SymbolKind};
use crate::tests::helpers::metadata_str;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("fixture extraction should succeed")
}

fn facts<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a crate::base::StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}


fn metadata<'a>(fact: &'a crate::base::StructuralFact, key: &str) -> Option<&'a Value> {
    fact.metadata.as_ref()?.get(key)
}

#[test]
fn code_behind_identity_fixture_certifies_component_navigation_and_http_facts() {
    let source = include_str!("../../../../../fixtures/extraction/razor/code-behind/Widget.razor");
    let results = extract("fixtures/extraction/razor/code-behind/Widget.razor", source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "Widget" && symbol.kind == SymbolKind::Class })
    );
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "Theme" && symbol.kind == SymbolKind::Property })
    );
    assert_eq!(facts(&results, "blazor.component_reference.v1").len(), 1);
    assert_eq!(facts(&results, "razor.route_reference.v1").len(), 2);
    let requests = facts(&results, "http.client_request.v1");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        metadata_str(requests[0], "target_path"),
        Some("/api/widgets/current")
    );

    let code_behind_source =
        include_str!("../../../../../fixtures/extraction/razor/code-behind/Widget.razor.cs");
    let code_behind = extract(
        "fixtures/extraction/razor/code-behind/Widget.razor.cs",
        code_behind_source,
    );
    assert!(
        code_behind
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "Widget" && symbol.kind == SymbolKind::Class })
    );
    assert!(
        code_behind
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "Theme" && symbol.kind == SymbolKind::Property })
    );
}

#[test]
fn imports_fixture_has_namespace_inputs_without_component_identity() {
    let source = include_str!("../../../../../fixtures/extraction/razor/imports/_Imports.razor");
    let results = extract("fixtures/extraction/razor/imports/_Imports.razor", source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Sample.Components")
    );
    assert!(!results.symbols.iter().any(|symbol| {
        symbol.kind == SymbolKind::Class
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("type"))
                .and_then(|value| value.as_str())
                == Some("razor-component")
    }));
}

#[test]
fn scoped_asset_fixture_keeps_adjacent_inputs_independently_extractable() {
    let razor_source =
        include_str!("../../../../../fixtures/extraction/razor/scoped-assets/ScopedPanel.razor");
    let css_source = include_str!(
        "../../../../../fixtures/extraction/razor/scoped-assets/ScopedPanel.razor.css"
    );
    let razor = extract(
        "fixtures/extraction/razor/scoped-assets/ScopedPanel.razor",
        razor_source,
    );
    let css = extract(
        "fixtures/extraction/razor/scoped-assets/ScopedPanel.razor.css",
        css_source,
    );

    assert!(
        razor
            .symbols
            .iter()
            .any(|symbol| symbol.name == "ScopedPanel")
    );
    assert_eq!(facts(&razor, "blazor.component_reference.v1").len(), 1);
    assert!(
        css.symbols
            .iter()
            .any(|symbol| symbol.name == ".scoped-panel")
    );
    assert_eq!(facts(&css, "css.selector_rule.v1").len(), 1);
}

#[test]
fn constrained_typeparam_fixture_preserves_constraint_and_generic_reference() {
    let source =
        include_str!("../../../../../fixtures/extraction/razor/typeparam/GenericList.razor");
    let results = extract(
        "fixtures/extraction/razor/typeparam/GenericList.razor",
        source,
    );

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "IEntity" && identifier.kind == IdentifierKind::TypeUsage
    }));
    let component = facts(&results, "blazor.component_reference.v1");
    assert_eq!(component.len(), 1);
    assert_eq!(metadata_str(component[0], "tag"), Some("DataGrid"));
    assert_eq!(
        metadata(component[0], "generic_arguments").and_then(Value::as_array),
        Some(&vec![serde_json::json!({
            "name": "TGridItem",
            "value": "TItem"
        })])
    );
}

#[test]
fn rendermode_fixture_certifies_page_component_and_cascading_property() {
    let source = include_str!(
        "../../../../../fixtures/extraction/razor/rendermode/InteractiveDashboard.razor"
    );
    let results = extract(
        "fixtures/extraction/razor/rendermode/InteractiveDashboard.razor",
        source,
    );

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert!(results.symbols.iter().any(|symbol| {
        symbol.name == "InteractiveDashboard" && symbol.kind == SymbolKind::Class
    }));
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "Theme" && symbol.kind == SymbolKind::Property })
    );
    assert_eq!(facts(&results, "razor.page_directive.v1").len(), 1);
    assert_eq!(facts(&results, "blazor.component_reference.v1").len(), 1);
}
