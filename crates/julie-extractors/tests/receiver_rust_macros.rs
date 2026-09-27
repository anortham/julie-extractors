use std::path::Path;

use julie_extractors::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

fn extract_rust(path: &str, source: &str) -> julie_extractors::ExtractionResults {
    extract_canonical_for_language_at("rust", path, source, Path::new("."), ExtractionLevel::Full)
        .unwrap()
}

#[test]
fn item_macro_calls_keep_their_canonical_receivers() {
    let source = include_str!("../../../fixtures/extraction/rust/items_and_macros/source.rs");
    let results = extract_rust("items_and_macros.rs", source);
    let receivers: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "new" && identifier.kind == IdentifierKind::Call)
        .filter_map(|identifier| identifier.metadata.as_ref()?.get("receiver")?.as_str())
        .collect();

    assert!(receivers.contains(&"Mutex"));
    assert!(receivers.contains(&"RefCell"));
}

#[test]
fn expression_macro_calls_keep_their_canonical_receivers() {
    let source = include_str!("../../../fixtures/extraction/rust/trait_impl/source.rs");
    let results = extract_rust("trait_impl.rs", source);
    let call = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "run" && identifier.kind == IdentifierKind::Call)
        .unwrap();

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("self")
    );
}

#[test]
fn nested_expression_macros_keep_receiver_spans_and_qualifiers() {
    let source = "fn run(service: Service) { outer!(inner!(service.client.send())); }";
    let results = extract_rust("nested_macro_receivers.rs", source);
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "send" && identifier.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].start_byte, calls[0].end_byte),
        (
            source.find("send").unwrap() as u32,
            (source.find("send").unwrap() + 4) as u32
        )
    );
    assert_eq!(
        calls[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("client")
    );
    assert_eq!(
        calls[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(serde_json::Value::as_str),
        Some("service")
    );
}

#[test]
fn expression_rooted_macro_receivers_remain_unset() {
    let source = "fn run() { write!(f, \"{}\", factory().service.send()); write!(f, \"{}\", items[index].send()); }";
    let results = extract_rust("macro_receivers.rs", source);
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "send" && identifier.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|identifier| {
        identifier.metadata.as_ref().is_none_or(|metadata| {
            !metadata.contains_key("receiver") && !metadata.contains_key("receiver_qualifier")
        })
    }));
}

#[test]
fn generic_method_calls_emit_once_without_hiding_member_reads() {
    let source = "fn run(text: &str) { text.parse::<u32>(); let _ = text.parse; }";
    let results = extract_rust("generic_method.rs", source);
    let calls = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "parse" && identifier.kind == IdentifierKind::Call)
        .count();
    let member_reads = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.name == "parse" && identifier.kind == IdentifierKind::MemberAccess
        })
        .count();

    assert_eq!((calls, member_reads), (1, 1));
}
