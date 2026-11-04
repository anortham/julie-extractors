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

fn annotation_keys(symbol: &crate::base::Symbol) -> Vec<&str> {
    let mut keys: Vec<_> = symbol
        .annotations
        .iter()
        .map(|annotation| annotation.annotation_key.as_str())
        .collect();
    keys.sort_unstable();
    keys
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

#[test]
fn razor_embedded_annotations_follow_extraction_levels_and_keep_host_offsets() {
    let source = r#"@page "/worker"

@code {
    [Parameter]
    public string Title { get; set; } = "Worker";

    [Authorize]
    private int Run(int id)
    {
        return id + 1;
    }
}"#;
    let property_source = "[Parameter]\n    public string Title { get; set; } = \"Worker\";";
    let method_source =
        "[Authorize]\n    private int Run(int id)\n    {\n        return id + 1;\n    }";
    let property_start = source.find(property_source).unwrap();
    let method_start = source.find(method_source).unwrap();

    for (level, has_annotations) in [
        (crate::ExtractionLevel::Symbols, false),
        (crate::ExtractionLevel::Facts, false),
        (crate::ExtractionLevel::References, true),
        (crate::ExtractionLevel::Full, true),
    ] {
        let results = crate::pipeline::extract_canonical_at(
            "src/Worker.razor",
            source,
            std::path::Path::new("/tmp/test"),
            level,
        )
        .expect("canonical Razor extraction should succeed");
        assert!(
            results.parse_diagnostics.is_empty(),
            "{:#?}",
            results.parse_diagnostics
        );

        let title = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Title" && symbol.kind == SymbolKind::Property)
            .expect("Title property should exist");
        let run = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Run" && symbol.kind == SymbolKind::Method)
            .expect("Run method should exist");
        let title_keys = if has_annotations {
            vec!["parameter"]
        } else {
            Vec::new()
        };
        let run_keys = if has_annotations {
            vec!["authorize"]
        } else {
            Vec::new()
        };

        assert_eq!(annotation_keys(title), title_keys, "{level:?}");
        assert_eq!(annotation_keys(run), run_keys, "{level:?}");
        assert_eq!(title.start_byte as usize, property_start);
        assert_eq!(
            source.get(title.start_byte as usize..title.end_byte as usize),
            Some(property_source)
        );
        assert_eq!(run.start_byte as usize, method_start);
        assert_eq!(
            source.get(run.start_byte as usize..run.end_byte as usize),
            Some(method_source)
        );
    }
}
