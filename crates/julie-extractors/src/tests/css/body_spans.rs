use crate::base::{ExtractionResults, Symbol};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("styles/bodies.css", source, Path::new("/repo")).expect("CSS extraction")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn body<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    symbol
        .body_span
        .map(|span| &source[span.start_byte as usize..span.end_byte as usize])
}

#[test]
fn css_rule_at_rule_and_keyframes_bodies_keep_exact_nested_source_spans() {
    let source = concat!(
        "@charset \"UTF-8\";\r\n",
        "/* café */\r\n",
        "@import \"theme.css\";\r\n",
        ".card { color: teal; }\r\n",
        ".empty {}\r\n",
        "@media (min-width: 40rem) { .nested { color: rebeccapurple; } }\r\n",
        "@keyframes pulse { 50% { opacity: .5; } }\r\n",
    );
    let results = extract(source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    assert_eq!(
        body(source, symbol(&results, ".card")),
        Some("{ color: teal; }")
    );
    assert_eq!(body(source, symbol(&results, ".empty")), Some("{}"));

    let container = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("type"))
                .and_then(serde_json::Value::as_str)
                == Some("media-rule")
        })
        .expect("media rule symbol");
    assert_eq!(
        body(source, container),
        Some("{ .nested { color: rebeccapurple; } }")
    );
    let nested = symbol(&results, ".nested");
    assert_eq!(nested.parent_id.as_deref(), Some(container.id.as_str()));
    assert_eq!(
        body(source, symbol(&results, "@keyframes pulse")),
        Some("{ 50% { opacity: .5; } }")
    );

    let imports: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == crate::base::SymbolKind::Import)
        .collect();
    assert_eq!(imports.len(), 1);
    assert_eq!(body(source, imports[0]), None);
    assert_eq!(imports[0].body_hash, None);

    let charset = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("atRuleType"))
                .and_then(serde_json::Value::as_str)
                == Some("charset")
        })
        .expect("charset symbol");
    assert_eq!(body(source, charset), None);
    assert_eq!(charset.body_hash, None);
}
