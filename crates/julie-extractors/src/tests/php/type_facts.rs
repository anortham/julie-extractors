use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::php::PhpExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, PhpExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "type_facts.php".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, PhpExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PhpExtractor::new(
        "php".to_string(),
        "type_facts.php".to_string(),
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
    extractor: &'a PhpExtractor,
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

fn no_fact(extractor: &PhpExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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
fn typed_function_parameters_record_single_base_names() {
    let source = r#"<?php
function f(?Foo $a, Foo|Bar $b, \App\Foo $c, Foo ...$rest) {}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let a = symbol(&symbols, "a", SymbolKind::Variable);
    let b = symbol(&symbols, "b", SymbolKind::Variable);
    let c = symbol(&symbols, "c", SymbolKind::Variable);
    let rest = symbol(&symbols, "rest", SymbolKind::Variable);
    assert_eq!(role(a), Some("parameter"));
    assert_eq!(role(b), Some("parameter"));
    assert_eq!(role(c), Some("parameter"));
    assert_eq!(role(rest), Some("parameter"));
    assert_eq!(a.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(b.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(c.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(rest.parent_id.as_deref(), Some(function.id.as_str()));
    let a_fact = fact(&extractor, &symbols, "a", SymbolKind::Variable);
    assert_eq!(a_fact.resolved_type, "Foo");
    assert!(!a_fact.is_inferred);
    assert_eq!(declared(a_fact), Some("?Foo"));
    no_fact(&extractor, &symbols, "b", SymbolKind::Variable);
    let c_fact = fact(&extractor, &symbols, "c", SymbolKind::Variable);
    assert_eq!(c_fact.resolved_type, "App\\Foo");
    assert!(!c_fact.is_inferred);
    assert_eq!(declared(c_fact), Some("\\App\\Foo"));
    let rest_fact = fact(&extractor, &symbols, "rest", SymbolKind::Variable);
    assert_eq!(rest_fact.resolved_type, "Foo");
    assert!(!rest_fact.is_inferred);
    assert_eq!(declared(rest_fact), None);
}

#[test]
fn promoted_constructor_parameter_keeps_property_and_adds_parameter() {
    let source = r#"<?php
class Service {
    public function __construct(private Foo $svc) {}
}
"#;
    let (symbols, extractor) = extract(source);
    let constructor = symbol(&symbols, "__construct", SymbolKind::Constructor);
    let class = symbol(&symbols, "Service", SymbolKind::Class);
    let property = symbol(&symbols, "svc", SymbolKind::Property);
    let parameter = symbol(&symbols, "svc", SymbolKind::Variable);
    assert_eq!(role(parameter), Some("parameter"));
    assert_eq!(
        parameter.parent_id.as_deref(),
        Some(constructor.id.as_str())
    );
    assert_eq!(property.parent_id.as_deref(), Some(class.id.as_str()));
    let property_fact = fact(&extractor, &symbols, "svc", SymbolKind::Property);
    assert_eq!(property_fact.resolved_type, "Foo");
    assert!(!property_fact.is_inferred);
    let parameter_fact = fact(&extractor, &symbols, "svc", SymbolKind::Variable);
    assert_eq!(parameter_fact.resolved_type, "Foo");
    assert!(!parameter_fact.is_inferred);
}

#[test]
fn new_expression_local_records_inferred_fact() {
    let source = r#"<?php
class Widget {}
function run() {
    $w = new Widget();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "w", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    let fact = fact(&extractor, &symbols, "w", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(fact.is_inferred);
}

#[test]
fn qualified_unknown_new_expression_records_inferred_fact() {
    let source = r#"<?php
function run() {
    $u = new \Vendor\Unknown();
}
"#;
    let (symbols, extractor) = extract(source);
    let _local = symbol(&symbols, "u", SymbolKind::Variable);
    let fact = fact(&extractor, &symbols, "u", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Vendor\\Unknown");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), Some("\\Vendor\\Unknown"));
}

#[test]
fn non_constructor_assignment_gets_symbol_without_fact() {
    let source = r#"<?php
function run() {
    $m = make();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "m", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    no_fact(&extractor, &symbols, "m", SymbolKind::Variable);
}

#[test]
fn typed_property_records_declared_fact() {
    let source = r#"<?php
class Box {
    public Foo $item;
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "item", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
}

#[test]
fn this_self_static_and_parent_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"<?php
class ServiceBase {}
class OrderService extends ServiceBase {
    public function process($other) {
        $this->run();
        self::make();
        static::build();
        parent::boot();
        $other->fetch();
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
    assert_eq!(call("run").receiver_type.as_deref(), Some("OrderService"));
    assert_eq!(call("make").receiver_type.as_deref(), Some("OrderService"));
    assert_eq!(call("build").receiver_type.as_deref(), Some("OrderService"));
    assert_eq!(call("boot").receiver_type.as_deref(), Some("ServiceBase"));
    assert_eq!(call("fetch").receiver_type, None);

    let pending = |name: &str| {
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .find(|p| p.target.terminal_name == name)
            .unwrap_or_else(|| panic!("missing structured pending for {name}"))
    };
    assert_eq!(
        pending("run").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("make").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("build").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("boot").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(pending("fetch").receiver_type, None);
}
