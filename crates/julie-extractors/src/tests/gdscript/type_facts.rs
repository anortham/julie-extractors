use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::gdscript::GDScriptExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, GDScriptExtractor) {
    let tree = init_parser(source, "gdscript");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = GDScriptExtractor::new(
        "gdscript".to_string(),
        "type_facts.gd".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, Vec<Identifier>, GDScriptExtractor) {
    let tree = init_parser(source, "gdscript");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = GDScriptExtractor::new(
        "gdscript".to_string(),
        "type_facts.gd".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);
    (symbols, identifiers, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(
    extractor: &'a GDScriptExtractor,
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

fn no_fact(extractor: &GDScriptExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        !extractor.base.type_info.contains_key(&symbol.id),
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
fn typed_and_bare_parameters_record_facts_only_when_stated() {
    let source = r#"
class_name Sample
func f(x: Foo, y := 2, z):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Method);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    let y = symbol(&symbols, "y", SymbolKind::Variable);
    let z = symbol(&symbols, "z", SymbolKind::Variable);
    assert_eq!(role(x), Some("parameter"));
    assert_eq!(role(y), Some("parameter"));
    assert_eq!(role(z), Some("parameter"));
    assert_eq!(x.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(y.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(z.parent_id.as_deref(), Some(function.id.as_str()));
    let x_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);
    no_fact(&extractor, &symbols, "y", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "z", SymbolKind::Variable);
}

#[test]
fn constructor_parameters_parent_to_init() {
    let source = r#"
class_name Sample
func _init(start: Foo):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let ctor = symbol(&symbols, "_init", SymbolKind::Constructor);
    let start = symbol(&symbols, "start", SymbolKind::Variable);
    assert_eq!(role(start), Some("parameter"));
    assert_eq!(start.parent_id.as_deref(), Some(ctor.id.as_str()));
    let start_fact = fact(&extractor, &symbols, "start", SymbolKind::Variable);
    assert_eq!(start_fact.resolved_type, "Foo");
    assert!(!start_fact.is_inferred);
}

#[test]
fn declared_local_records_fact_and_variable_kind() {
    let source = r#"
class_name Sample
func run():
    var x: Foo = null
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let local = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(method.id.as_str()));
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn same_file_new_initializer_records_inferred_fact() {
    let source = r#"
class_name Sample
class Foo:
    pass
func run():
    var x := Foo.new()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(fact.is_inferred);
}

#[test]
fn unknown_imported_and_non_constructor_initializers_record_no_fact() {
    let source = r#"
class_name Sample
func run():
    var a = Unknown.new()
    var b = load("res://x.tscn").instantiate()
    var c = make()
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "a", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "b", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "c", SymbolKind::Variable);
}

#[test]
fn array_generic_local_records_base_name_and_declared_metadata() {
    let source = r#"
class_name Sample
func run():
    var items: Array[Foo]
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "items", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Array");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Array[Foo]"));
}

#[test]
fn class_level_variable_keeps_field_kind_and_records_fact() {
    let source = r#"
class_name Sample
var hp: int
func run():
    var local: Foo = null
"#;
    let (symbols, extractor) = extract(source);
    let class = symbol(&symbols, "Sample", SymbolKind::Class);
    let field = symbol(&symbols, "hp", SymbolKind::Field);
    let local = symbol(&symbols, "local", SymbolKind::Variable);
    assert_eq!(field.parent_id.as_deref(), Some(class.id.as_str()));
    let field_fact = fact(&extractor, &symbols, "hp", SymbolKind::Field);
    assert_eq!(field_fact.resolved_type, "int");
    assert!(!field_fact.is_inferred);
    assert_eq!(local.kind, SymbolKind::Variable);
}

#[test]
fn self_and_super_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"
class_name Foo
extends Node
func run():
    self.persist()
    super.restore()
    other.fetch()
class Bar extends Resource:
    func inner_run():
        self.persist()
        super.restore()
"#;
    let (_symbols, identifiers, extractor) = extract_calls(source);
    let calls = |name: &str| -> Vec<&Identifier> {
        identifiers
            .iter()
            .filter(|id| id.name == name && id.kind == IdentifierKind::Call)
            .collect()
    };
    let persist = calls("persist");
    let restore = calls("restore");
    let fetch = calls("fetch");
    assert_eq!(persist.len(), 2);
    assert_eq!(persist[0].receiver_type.as_deref(), Some("Foo"));
    assert_eq!(persist[1].receiver_type.as_deref(), Some("Bar"));
    assert_eq!(restore.len(), 2);
    assert_eq!(restore[0].receiver_type.as_deref(), Some("Node"));
    assert_eq!(restore[1].receiver_type.as_deref(), Some("Resource"));
    assert_eq!(fetch.len(), 1);
    assert_eq!(fetch[0].receiver_type, None);

    let pending = extractor.get_structured_pending_relationships();
    let pending_for = |name: &str, receiver: &str| {
        pending
            .iter()
            .find(|p| {
                p.target.terminal_name == name && p.target.receiver.as_deref() == Some(receiver)
            })
            .unwrap_or_else(|| panic!("missing pending {name} on {receiver}"))
    };
    assert_eq!(
        pending_for("persist", "self").receiver_type.as_deref(),
        Some("Foo")
    );
    assert_eq!(
        pending_for("restore", "super").receiver_type.as_deref(),
        Some("Node")
    );
    let bar_persist = pending
        .iter()
        .filter(|p| {
            p.target.terminal_name == "persist" && p.target.receiver.as_deref() == Some("self")
        })
        .collect::<Vec<_>>();
    assert!(
        bar_persist
            .iter()
            .any(|p| p.receiver_type.as_deref() == Some("Bar"))
    );
    let bar_restore = pending
        .iter()
        .filter(|p| {
            p.target.terminal_name == "restore" && p.target.receiver.as_deref() == Some("super")
        })
        .collect::<Vec<_>>();
    assert!(
        bar_restore
            .iter()
            .any(|p| p.receiver_type.as_deref() == Some("Resource"))
    );
    assert_eq!(pending_for("fetch", "other").receiver_type, None);
}

#[test]
fn function_local_const_is_variable_and_class_const_stays_constant() {
    let source = r#"
class_name Sample
const LIMIT: int = 3
func run():
    const LOCAL: int = 2
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let limit = symbol(&symbols, "LIMIT", SymbolKind::Constant);
    let local = symbol(&symbols, "LOCAL", SymbolKind::Variable);
    assert_eq!(limit.kind, SymbolKind::Constant);
    assert_eq!(local.parent_id.as_deref(), Some(method.id.as_str()));
    assert!(
        !symbols
            .iter()
            .any(|s| s.name == "LOCAL" && s.kind == SymbolKind::Constant)
    );
    let local_fact = fact(&extractor, &symbols, "LOCAL", SymbolKind::Variable);
    assert_eq!(local_fact.resolved_type, "int");
    assert!(!local_fact.is_inferred);
}

#[test]
fn qualified_new_initializer_records_no_fact() {
    let source = r#"
class_name Sample
class Foo:
    pass
func run():
    var direct := Foo.new()
    var nested = a.Foo.new()
    var chained = Foo.inner.new()
"#;
    let (symbols, extractor) = extract(source);
    let direct = fact(&extractor, &symbols, "direct", SymbolKind::Variable);
    assert_eq!(direct.resolved_type, "Foo");
    no_fact(&extractor, &symbols, "nested", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "chained", SymbolKind::Variable);
}
