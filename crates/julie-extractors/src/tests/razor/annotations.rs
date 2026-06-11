use super::extract_symbols;
use crate::base::AnnotationMarker;
use crate::base::SymbolKind;

fn annotation<'a>(symbol: &'a crate::base::Symbol, key: &str) -> &'a AnnotationMarker {
    symbol
        .annotations
        .iter()
        .find(|marker| marker.annotation_key == key)
        .unwrap_or_else(|| {
            panic!(
                "symbol `{}` missing annotation `{key}`; has: {:?}",
                symbol.name, symbol.annotations
            )
        })
}

#[test]
fn razor_embedded_csharp_attributes_normalize_on_methods_and_properties() {
    let razor_code = r#"@page "/worker"

@code {
    [Parameter]
    public string Title { get; set; } = "Worker";

    [Authorize]
    private int Run(int id)
    {
        return id + 1;
    }
}"#;

    let symbols = extract_symbols(razor_code);

    let title = symbols
        .iter()
        .find(|symbol| symbol.name == "Title" && symbol.kind == SymbolKind::Property)
        .expect("Title property should exist");
    assert_eq!(annotation(title, "parameter").annotation, "Parameter");
    assert_eq!(
        annotation(title, "parameter").raw_text.as_deref(),
        Some("Parameter")
    );

    let run = symbols
        .iter()
        .find(|symbol| symbol.name == "Run" && symbol.kind == SymbolKind::Method)
        .expect("Run method should exist");
    assert_eq!(annotation(run, "authorize").annotation, "Authorize");
    assert_eq!(
        annotation(run, "authorize").raw_text.as_deref(),
        Some("Authorize")
    );
}

#[test]
fn razor_class_attributes_normalize_when_declared_in_code_block() {
    let razor_code = r#"@code {
    [Route("/worker")]
    public partial class WorkerPage {
        public string Title { get; set; }
    }
}"#;

    let symbols = extract_symbols(razor_code);

    let worker_page = symbols
        .iter()
        .find(|symbol| symbol.name == "WorkerPage" && symbol.kind == SymbolKind::Class)
        .expect("WorkerPage class should exist");
    assert_eq!(annotation(worker_page, "route").annotation, "Route");
}
