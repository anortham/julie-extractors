use crate::ExtractionResults;
use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/Main.kt", source, Path::new("/repo"))
        .expect("canonical Kotlin extraction must succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("expected {kind:?} symbol {name}"))
}

fn assert_body(source: &str, symbol: &Symbol, expected: &str) {
    let span = symbol
        .body_span
        .expect("body-bearing symbol needs a body span");
    assert_eq!(
        source.get(span.start_byte as usize..span.end_byte as usize),
        Some(expected)
    );
    assert!(symbol.body_hash.is_some());
}

#[test]
fn constructor_property_function_type_parameters_have_no_body() {
    let source = r#"class Button(
    @Suppress("UNUSED") private val id: Int,
    val onClick: () -> Unit,
    label: String
)"#;
    let results = extract(source);
    let button = symbol(&results, "Button", SymbolKind::Class);
    let id = symbol(&results, "id", SymbolKind::Property);
    let on_click = symbol(&results, "onClick", SymbolKind::Property);

    assert_eq!(id.parent_id.as_deref(), Some(button.id.as_str()));
    assert!(id.body_span.is_none());
    assert!(id.body_hash.is_none());
    assert_eq!(id.annotations.len(), 1);
    assert_eq!(id.annotations[0].annotation_key, "suppress");
    assert_eq!(
        id.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("dataType"))
            .and_then(serde_json::Value::as_str),
        Some("Int")
    );
    assert_eq!(on_click.parent_id.as_deref(), Some(button.id.as_str()));
    assert_eq!(
        on_click.signature.as_deref(),
        Some("val onClick: () -> Unit")
    );
    assert!(on_click.body_span.is_none());
    assert!(on_click.body_hash.is_none());
}

#[test]
fn constructor_property_values_and_property_accessors_keep_their_bodies() {
    let source = r#"class Cache(val callback: () -> Unit = {}) {
    val entries = mutableMapOf<String, Int>()
    val listener: () -> Unit = {}
    var balance: Int = 0
        get() = field
        set(value) { field = value }
    val label: String
        get() = "ready"
}"#;
    let results = extract(source);
    let cache = symbol(&results, "Cache", SymbolKind::Class);
    let callback = symbol(&results, "callback", SymbolKind::Property);
    let entries = symbol(&results, "entries", SymbolKind::Property);
    let listener = symbol(&results, "listener", SymbolKind::Property);
    let balance = symbol(&results, "balance", SymbolKind::Property);
    let label = symbol(&results, "label", SymbolKind::Property);

    for property in [callback, entries, listener, balance, label] {
        assert_eq!(property.parent_id.as_deref(), Some(cache.id.as_str()));
    }
    assert_body(source, callback, "{}");
    assert_body(source, entries, "mutableMapOf<String, Int>()");
    assert_body(source, listener, "{}");
    assert_body(source, balance, "0");
    assert_body(source, label, "\"ready\"");

    let changed = source.replace("\"ready\"", "\"ready!\"");
    assert_ne!(
        label.body_hash,
        symbol(&extract(&changed), "label", SymbolKind::Property).body_hash
    );
}
