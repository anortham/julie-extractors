use crate::base::ExtractionResults;
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("source.html", source, Path::new("/repo")).expect("HTML extraction")
}

#[test]
fn test_factory_extracts_html_types() {
    let source = concat!(
        "<!doctype html>\n",
        "<html><head><title>Workers</title></head><body>\n",
        "<input id=\"login\" type=\"email\" name=\"username\">\n",
        "<button type=\"submit\" aria-label=\"save\">Save</button>\n",
        "<div id=\"note\" data-type=\"note\"></div>\n",
        "</body></html>\n",
    );
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());

    for name in ["html", "body", "input", "button", "div"] {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("missing HTML element symbol {name}"));
        let type_info = results
            .types
            .get(&symbol.id)
            .unwrap_or_else(|| panic!("missing type for HTML element {name}"));

        assert_eq!(type_info.resolved_type, "html-element");
        assert_eq!(type_info.language, "html");
        assert!(type_info.is_inferred);
    }

    assert!(results.symbols.iter().all(|symbol| {
        !["login", "email", "username", "submit", "save", "note"].contains(&symbol.name.as_str())
    }));
    assert!(
        results
            .types
            .keys()
            .all(|symbol_id| results.symbols.iter().any(|symbol| &symbol.id == symbol_id))
    );
}
