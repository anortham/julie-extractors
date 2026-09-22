use crate::base::{Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> Vec<Symbol> {
    extract_canonical("config.json", source, Path::new("/tmp/test"))
        .unwrap()
        .symbols
}

fn find<'a>(symbols: &'a [Symbol], name: &str, line: u32) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.start_line == line)
        .unwrap_or_else(|| panic!("missing {name} on line {line}: {symbols:#?}"))
}

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

#[test]
fn object_elements_of_an_array_become_indexed_containers() {
    let source = r#"{
  "configurations": [
    { "name": "Launch API", "type": "coreclr" },
    { "name": "Attach", "type": "coreclr" }
  ]
}"#;
    let symbols = extract(source);

    let configurations = find(&symbols, "configurations", 2);
    let first = find(&symbols, "[0]", 3);
    let second = find(&symbols, "[1]", 4);
    assert_eq!(first.kind, SymbolKind::Module);
    assert_eq!(first.parent_id.as_ref(), Some(&configurations.id));
    assert_eq!(second.parent_id.as_ref(), Some(&configurations.id));
    assert_eq!(
        find(&symbols, "name", 3).parent_id.as_ref(),
        Some(&first.id)
    );
    assert_eq!(
        find(&symbols, "name", 4).parent_id.as_ref(),
        Some(&second.id)
    );
}

#[test]
fn body_spans_cover_container_values_and_skip_scalars() {
    let source = r#"{
  "paths": { "/users/{id}": { "get": { "operationId": "getUser" } } },
  "tags": ["alpha", "beta"],
  "users": [ { "name": "a" } ],
  "route": "/items/{itemId}",
  "formula": "sum(a, b)",
  "emptyList": []
}"#;
    let symbols = extract(source);

    assert_eq!(
        body_text(source, find(&symbols, "/users/{id}", 2)),
        Some(r#"{ "get": { "operationId": "getUser" } }"#)
    );
    assert_eq!(
        body_text(source, find(&symbols, "tags", 3)),
        Some(r#"["alpha", "beta"]"#)
    );
    assert_eq!(
        body_text(source, find(&symbols, "users", 4)),
        Some(r#"[ { "name": "a" } ]"#)
    );
    assert_eq!(
        body_text(source, find(&symbols, "[0]", 4)),
        Some(r#"{ "name": "a" }"#)
    );
    assert_eq!(
        body_text(source, find(&symbols, "emptyList", 7)),
        Some("[]")
    );
    for scalar in [("route", 5), ("formula", 6)] {
        let symbol = find(&symbols, scalar.0, scalar.1);
        assert!(symbol.body_span.is_none() && symbol.body_hash.is_none());
    }
}
