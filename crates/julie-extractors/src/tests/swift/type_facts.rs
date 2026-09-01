use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::swift::SwiftExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, SwiftExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = SwiftExtractor::new(
        "swift".to_string(),
        "type_facts.swift".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, SwiftExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = SwiftExtractor::new(
        "swift".to_string(),
        "type_facts.swift".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
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
    extractor: &'a SwiftExtractor,
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

fn no_fact(extractor: &SwiftExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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

#[test]
fn typed_parameters_become_symbols_with_declared_facts() {
    let source = r#"
class Box {
    init(seed: Bar) {}
}
func f(x: Foo, y: inout Bar) {}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    let y = symbol(&symbols, "y", SymbolKind::Variable);
    assert_eq!(x.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(y.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(role(x), Some("parameter"));
    assert_eq!(role(y), Some("parameter"));
    let x_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);
    let y_fact = fact(&extractor, &symbols, "y", SymbolKind::Variable);
    assert_eq!(y_fact.resolved_type, "Bar");
    assert!(!y_fact.is_inferred);

    let constructor = symbol(&symbols, "init", SymbolKind::Constructor);
    let seed = symbol(&symbols, "seed", SymbolKind::Variable);
    assert_eq!(seed.parent_id.as_deref(), Some(constructor.id.as_str()));
    assert_eq!(role(seed), Some("parameter"));
    let seed_fact = fact(&extractor, &symbols, "seed", SymbolKind::Variable);
    assert_eq!(seed_fact.resolved_type, "Bar");
    assert!(!seed_fact.is_inferred);
}

#[test]
fn optional_local_records_base_name() {
    let source = r#"
func run() {
    let x: Foo? = nil
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    let local_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(local_fact.resolved_type, "Foo");
    assert!(!local_fact.is_inferred);
    assert_eq!(declared(local_fact), Some("Foo?"));
}

#[test]
fn same_file_constructor_call_records_inferred_fact() {
    let source = r#"
class Foo {}
func run() {
    let x = Foo()
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    let local_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(local_fact.resolved_type, "Foo");
    assert!(local_fact.is_inferred);
}

#[test]
fn constructor_negatives_keep_symbol_without_fact() {
    let source = r#"
class Foo {}
func makeFoo() -> Foo { return Foo() }
func run() {
    let a = Unknown()
    let b = UIKit.UIView()
    let c = makeFoo()
}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "a", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "b", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "c", SymbolKind::Variable);
}

#[test]
fn array_local_records_no_fact() {
    let source = r#"
func run() {
    var items: [Foo] = []
}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "items", SymbolKind::Variable);
}

#[test]
fn stored_property_records_declared_fact() {
    let source = r#"
class Box {
    let value: Foo
}
"#;
    let (symbols, extractor) = extract(source);
    let class = symbol(&symbols, "Box", SymbolKind::Class);
    let property = symbol(&symbols, "value", SymbolKind::Property);
    assert_eq!(property.parent_id.as_deref(), Some(class.id.as_str()));
    let property_fact = fact(&extractor, &symbols, "value", SymbolKind::Property);
    assert_eq!(property_fact.resolved_type, "Foo");
    assert!(!property_fact.is_inferred);
}

#[test]
fn self_and_super_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"
class ServiceBase {}
class OrderService: ServiceBase {
    func process(other: Worker) {
        self.persist()
        super.restore()
        other.fetch()
    }
}
class Solo {
    func run() {
        super.absent()
    }
}
extension OrderService {
    func extra() {
        self.persist()
    }
}
"#;
    let (_symbols, extractor) = extract_calls(source);
    let call = |name: &str| {
        extractor
            .base
            .identifiers
            .iter()
            .find(|id| id.name == name && id.kind == IdentifierKind::Call)
            .unwrap_or_else(|| panic!("missing call identifier {name}"))
    };
    assert_eq!(
        call("persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        call("restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(call("fetch").receiver_type, None);
    assert_eq!(call("absent").receiver_type, None);
    assert!(
        extractor
            .base
            .identifiers
            .iter()
            .filter(|id| id.name == "persist" && id.kind == IdentifierKind::Call)
            .all(|id| id.receiver_type.as_deref() == Some("OrderService"))
    );

    let pending = |name: &str| {
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .find(|p| p.target.terminal_name == name)
            .unwrap_or_else(|| panic!("missing structured pending for {name}"))
    };
    assert_eq!(
        pending("persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(pending("fetch").receiver_type, None);
    assert_eq!(pending("absent").receiver_type, None);
    assert!(
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|p| p.target.terminal_name == "persist")
            .all(|p| p.receiver_type.as_deref() == Some("OrderService"))
    );
}

#[test]
fn super_call_receiver_type_reduces_generic_base_to_name() {
    let source = r#"
class Base<T> {}
class Foo: Base<Int> {
    func run() {
        super.restore()
    }
}
"#;
    let (_symbols, extractor) = extract_calls(source);
    let restore = extractor
        .base
        .identifiers
        .iter()
        .find(|id| id.name == "restore" && id.kind == IdentifierKind::Call)
        .expect("missing call identifier restore");
    assert_eq!(restore.receiver_type.as_deref(), Some("Base"));
    let pending = extractor
        .get_structured_pending_relationships()
        .into_iter()
        .find(|p| p.target.terminal_name == "restore")
        .expect("missing structured pending for restore");
    assert_eq!(pending.receiver_type.as_deref(), Some("Base"));
}

#[test]
fn qualified_type_records_namespace_qualified_base_name() {
    let source = r#"
func run() {
    let plain: Foo.Bar = make()
    let generic: Foo.Bar<Int>? = nil
}
"#;
    let (symbols, extractor) = extract(source);
    let plain = fact(&extractor, &symbols, "plain", SymbolKind::Variable);
    assert_eq!(plain.resolved_type, "Foo.Bar");
    assert!(!plain.is_inferred);
    assert_eq!(declared(plain), None);
    let generic = fact(&extractor, &symbols, "generic", SymbolKind::Variable);
    assert_eq!(generic.resolved_type, "Foo.Bar");
    assert_eq!(declared(generic), Some("Foo.Bar<Int>?"));
}

#[test]
fn artifact_types_never_carry_non_base_names() {
    let source = r#"
class Box {
    var items: [Foo] = []
    var lookup: [String: Foo] = [:]
    var pair: (Foo, Bar)? = nil
    var handler: () -> Void = {}
    var optional: Foo? = nil
    var generic: Array<Foo>? = []
    var qualified: Foo.Bar = Foo.Bar()
}
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let results = crate::factory::extract_symbols_and_relationships(
        &tree,
        "type_facts.swift",
        source,
        "swift",
        &workspace_root,
    )
    .unwrap();
    let resolved = |name: &str| -> Option<String> {
        let symbol = symbol(&results.symbols, name, SymbolKind::Property);
        results
            .types
            .get(&symbol.id)
            .map(|info| info.resolved_type.clone())
    };
    assert_eq!(resolved("items"), None);
    assert_eq!(resolved("lookup"), None);
    assert_eq!(resolved("pair"), None);
    assert_eq!(resolved("handler"), None);
    assert_eq!(resolved("optional").as_deref(), Some("Foo"));
    assert_eq!(resolved("generic").as_deref(), Some("Array"));
    assert_eq!(resolved("qualified").as_deref(), Some("Foo.Bar"));
    for info in results.types.values() {
        let value = info.resolved_type.as_str();
        assert!(
            !value.contains(['[', '(', '<', '?', '!', ' ']),
            "non-base resolved_type {value}"
        );
    }
}
