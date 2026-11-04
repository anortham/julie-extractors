use std::path::Path;

use crate::base::{ExtractionResults, Symbol, SymbolKind};
use crate::extract_canonical;

const HTML: &str = include_str!("../../../../../fixtures/extraction/html/annotations/source.html");

fn extract(source: &str) -> ExtractionResults {
    extract_canonical(
        "fixtures/extraction/html/annotations/source.html",
        source,
        Path::new("/repo"),
    )
    .expect("HTML extraction")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {name} {kind:?}"))
}

fn annotations(symbol: &Symbol) -> Vec<(&str, &str, Option<&str>)> {
    symbol
        .annotations
        .iter()
        .map(|annotation| {
            (
                annotation.annotation.as_str(),
                annotation.annotation_key.as_str(),
                annotation.raw_text.as_deref(),
            )
        })
        .collect()
}

#[test]
fn babel_script_annotations_keep_embedded_owners_and_host_spans() {
    let results = extract(HTML);

    assert!(results.parse_diagnostics.is_empty());

    let expected = [
        (
            "Registry",
            SymbolKind::Class,
            vec![("singleton", "singleton", Some("singleton"))],
            (8, 0, 13, 1),
            "@singleton\nclass Registry {\n  @observable items = [];\n  @Input() name;\n  @action.bound add(item) {}\n}",
        ),
        (
            "items",
            SymbolKind::Field,
            vec![("observable", "observable", Some("observable"))],
            (10, 2, 10, 24),
            "@observable items = []",
        ),
        (
            "name",
            SymbolKind::Field,
            vec![("Input", "input", Some("Input()"))],
            (11, 2, 11, 15),
            "@Input() name",
        ),
        (
            "add",
            SymbolKind::Method,
            vec![("action.bound", "action.bound", Some("action.bound"))],
            (12, 2, 12, 28),
            "@action.bound add(item) {}",
        ),
    ];

    let registry = symbol(&results, "Registry", SymbolKind::Class);

    for (name, kind, expected_annotations, coordinates, expected_source) in expected {
        let symbol = symbol(&results, name, kind);
        assert_eq!(symbol.language, "jsx");
        assert_eq!(annotations(symbol), expected_annotations);
        assert_eq!(
            (
                symbol.start_line,
                symbol.start_column,
                symbol.end_line,
                symbol.end_column,
            ),
            coordinates
        );
        let span = &HTML[symbol.start_byte as usize..symbol.end_byte as usize];
        assert_eq!(span, expected_source);
        if name != "Registry" {
            assert_eq!(symbol.parent_id.as_deref(), Some(registry.id.as_str()));
        }
    }

    let add = symbol(&results, "add", SymbolKind::Method);
    let body = add.body_span.expect("embedded method body span");
    assert_eq!(
        &HTML[body.start_byte as usize..body.end_byte as usize],
        "{}"
    );
    assert_eq!(
        (
            body.start_line,
            body.start_column,
            body.end_line,
            body.end_column,
        ),
        (12, 26, 12, 28)
    );

    for (name, kind, language) in [
        ("Ordinary", SymbolKind::Class, "javascript"),
        ("plainField", SymbolKind::Field, "javascript"),
        ("ordinaryMethod", SymbolKind::Method, "javascript"),
        ("Plain", SymbolKind::Class, "jsx"),
    ] {
        let symbol = symbol(&results, name, kind);
        assert_eq!(symbol.language, language);
        assert!(symbol.annotations.is_empty(), "{name}");
    }
}

#[test]
fn babel_annotation_spans_keep_host_byte_coordinates_after_unicode_crlf() {
    let source = "<!-- π\r\n  separator -->\r\n<script type=\"text/babel\">\r\n@singleton\r\nclass Coordinates {\r\n  @observable label = \"λ\";\r\n}\r\n</script>\r\n";
    let results = extract(source);
    let class = symbol(&results, "Coordinates", SymbolKind::Class);
    let field = symbol(&results, "label", SymbolKind::Field);
    let class_start = source.find("@singleton").unwrap();
    let field_start = source.find("@observable").unwrap();

    assert!(results.parse_diagnostics.is_empty());
    assert_eq!(
        &source[class.start_byte as usize..class.end_byte as usize],
        "@singleton\r\nclass Coordinates {\r\n  @observable label = \"λ\";\r\n}"
    );
    assert_eq!(class.start_byte as usize, class_start);
    assert_eq!((class.start_line, class.start_column), (4, 0));
    assert_eq!(
        &source[field.start_byte as usize..field.end_byte as usize],
        "@observable label = \"λ\""
    );
    assert_eq!(field.start_byte as usize, field_start);
    assert_eq!((field.start_line, field.start_column), (6, 2));
    assert_eq!(
        annotations(field),
        [("observable", "observable", Some("observable"))]
    );
}
