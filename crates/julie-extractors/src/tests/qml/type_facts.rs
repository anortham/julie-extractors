use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::qml::QmlExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn extract(source: &str) -> (Vec<Symbol>, QmlExtractor) {
    extract_with_path(source, "test.qml")
}

fn extract_with_path(source: &str, file_path: &str) -> (Vec<Symbol>, QmlExtractor) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_qmljs::LANGUAGE.into())
        .expect("load QML grammar");
    let tree = parser.parse(source, None).expect("parse QML");
    let mut extractor = QmlExtractor::new(
        "qml".to_string(),
        file_path.to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str, file_path: &str) -> (Vec<Symbol>, QmlExtractor) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_qmljs::LANGUAGE.into())
        .expect("load QML grammar");
    let tree = parser.parse(source, None).expect("parse QML");
    let mut extractor = QmlExtractor::new(
        "qml".to_string(),
        file_path.to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let _identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(
    extractor: &'a QmlExtractor,
    symbols: &[Symbol],
    name: &str,
    kind: SymbolKind,
) -> &'a TypeInfo {
    let symbol = symbol(symbols, name, kind);
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {name}"))
}

fn no_fact(extractor: &QmlExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
        "unexpected type fact for {name}"
    );
}

fn declared(fact: &TypeInfo) -> Option<&str> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
}

fn parameter_symbols<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
    symbols
        .iter()
        .filter(|s| {
            s.name == name
                && s.metadata
                    .as_ref()
                    .and_then(|m| m.get("role"))
                    .map(|role| role == &serde_json::json!("parameter"))
                    .unwrap_or(false)
        })
        .collect()
}

#[test]
fn function_parameters_become_symbols_without_facts() {
    let source = r#"
Item {
    function format(title, count) {
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "format", SymbolKind::Function);
    for name in ["title", "count"] {
        let params = parameter_symbols(&symbols, name);
        assert_eq!(params.len(), 1, "expected one `{name}` parameter symbol");
        let param = params[0];
        assert_eq!(param.kind, SymbolKind::Variable);
        assert_eq!(role(param), Some("parameter"));
        assert_eq!(param.parent_id.as_deref(), Some(function.id.as_str()));
        assert!(extractor.base.type_info.get(&param.id).is_none());
    }
}

#[test]
fn typed_property_records_declared_fact() {
    let source = r#"
Item {
    property LocalCard card
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "card", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "LocalCard");
    assert!(!fact.is_inferred);
    assert_eq!(fact.language, "qml");
    assert_eq!(declared(fact), None);
}

#[test]
fn generic_list_property_records_list_with_declared_metadata() {
    let source = r#"
Item {
    property list<Item> rows
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "rows", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "list");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("list<Item>"));
}

#[test]
fn alias_property_records_no_fact() {
    let source = r#"
Item {
    property string title: "Worker"
    property alias label: title
}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "label", SymbolKind::Property);
}

#[test]
fn var_property_records_var() {
    let source = r#"
Item {
    property var payload
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "payload", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "var");
    assert!(!fact.is_inferred);
}

#[test]
fn new_expression_local_records_inferred_fact() {
    let source = r#"
Item {
    function seed() {
        let card = new LocalCard()
        let d = new Date()
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "seed", SymbolKind::Function);
    for (name, ty) in [("card", "LocalCard"), ("d", "Date")] {
        let local = symbol(&symbols, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
        let fact = fact(&extractor, &symbols, name, SymbolKind::Variable);
        assert_eq!(fact.resolved_type, ty);
        assert!(fact.is_inferred);
        assert_eq!(fact.language, "qml");
    }
}

#[test]
fn namespaced_new_expression_records_nothing() {
    let source = r#"
Item {
    function seed() {
        let graph = new ns.GraphTraversal()
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let graph = symbol(&symbols, "graph", SymbolKind::Variable);
    assert_eq!(
        graph.parent_id.as_deref(),
        Some(symbol(&symbols, "seed", SymbolKind::Function).id.as_str())
    );
    no_fact(&extractor, &symbols, "graph", SymbolKind::Variable);
}

#[test]
fn non_constructor_call_local_records_nothing() {
    let source = r#"
Item {
    function seed() {
        let n = compute()
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let local = symbol(&symbols, "n", SymbolKind::Variable);
    assert_eq!(
        local.parent_id.as_deref(),
        Some(symbol(&symbols, "seed", SymbolKind::Function).id.as_str())
    );
    no_fact(&extractor, &symbols, "n", SymbolKind::Variable);
}

#[test]
fn id_and_this_calls_record_enclosing_component_as_receiver_type() {
    let source = r#"
Item {
    id: root
    function run() {
        root.format(x)
        this.helper()
        other.format(x)
    }
}
"#;
    let (_, extractor) = extract_calls(source, "Widget.qml");
    let identifiers = extractor.base.identifiers.clone();
    let format_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "format" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(format_calls.len(), 2);
    assert_eq!(format_calls[0].receiver_type.as_deref(), Some("Widget"));
    assert_eq!(format_calls[1].receiver_type, None);

    let helper_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "helper" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(helper_calls.len(), 1);
    assert_eq!(helper_calls[0].receiver_type.as_deref(), Some("Widget"));

    let pending = extractor.get_structured_pending_relationships();
    let pending_for = |receiver: &str| {
        pending
            .iter()
            .find(|p| {
                p.target.terminal_name == "format" && p.target.receiver.as_deref() == Some(receiver)
            })
            .unwrap_or_else(|| panic!("missing pending format on {receiver}"))
    };
    assert_eq!(
        pending_for("root").receiver_type.as_deref(),
        Some("Widget")
    );
    assert_eq!(pending_for("other").receiver_type, None);
    let helper_pending = pending
        .iter()
        .find(|p| p.target.terminal_name == "helper")
        .expect("missing pending helper");
    assert_eq!(helper_pending.receiver_type.as_deref(), Some("Widget"));
}
