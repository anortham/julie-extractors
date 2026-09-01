use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::rust::RustExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, RustExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "type_facts.rs".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(extractor: &'a RustExtractor, symbol: &Symbol) -> &'a TypeInfo {
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {}", symbol.name))
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
fn typed_local_records_declared_type_without_inference() {
    let source = r#"
fn run() {
    let traversal: GraphTraversal = make();
}
"#;
    let (symbols, extractor) = extract(source);
    let run = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "traversal", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(role(local), Some("local"));
    assert_eq!(
        local.signature.as_deref(),
        Some("let traversal: GraphTraversal")
    );
    let fact = fact(&extractor, local);
    assert_eq!(fact.resolved_type, "GraphTraversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn generic_typed_local_records_base_name_with_declared_metadata() {
    let source = r#"
fn run() {
    let items: Vec<String> = Vec::new();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, symbol(&symbols, "items", SymbolKind::Variable));
    assert_eq!(fact.resolved_type, "Vec");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Vec<String>"));
}

#[test]
fn path_typed_local_records_final_segment_with_full_path_declared() {
    let source = r#"
fn run() {
    let traversal: graph::Traversal = make();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("graph::Traversal"));
}

#[test]
fn generic_path_typed_local_records_final_segment() {
    let source = r#"
fn run() {
    let traversal: graph::Traversal<u32> = make();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert_eq!(declared(fact), Some("graph::Traversal<u32>"));
}

#[test]
fn new_call_local_records_constructed_type_as_inferred() {
    let source = r#"
fn run() {
    let traversal = Traversal::new();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn path_new_call_local_records_final_segment_as_inferred() {
    let source = r#"
fn run() {
    let traversal = graph::Traversal::new();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), Some("graph::Traversal"));
}

#[test]
fn struct_expression_local_records_type_as_inferred() {
    let source = r#"
fn run() {
    let config = Config { retries: 3 };
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, symbol(&symbols, "config", SymbolKind::Variable));
    assert_eq!(fact.resolved_type, "Config");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn non_new_call_local_records_no_fact() {
    let source = r#"
fn run() {
    let value = build();
}
"#;
    let (symbols, extractor) = extract(source);
    let local = symbol(&symbols, "value", SymbolKind::Variable);
    assert_eq!(local.signature.as_deref(), Some("let value"));
    assert!(!extractor.base.type_info.contains_key(&local.id));
}

#[test]
fn typed_parameter_becomes_symbol_with_declared_type_fact() {
    let source = r#"
fn run(count: u32, graph: Traversal) {
}
"#;
    let (symbols, extractor) = extract(source);
    let run = symbol(&symbols, "run", SymbolKind::Function);
    let param = symbol(&symbols, "graph", SymbolKind::Variable);
    assert_eq!(param.parent_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(role(param), Some("parameter"));
    assert_eq!(param.signature.as_deref(), Some("graph: Traversal"));
    let fact = fact(&extractor, param);
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
    let count = symbol(&symbols, "count", SymbolKind::Variable);
    assert_eq!(
        extractor
            .base
            .type_info
            .get(&count.id)
            .map(|f| f.resolved_type.as_str()),
        Some("u32")
    );
}

#[test]
fn reference_parameter_records_base_type_with_declared_metadata() {
    let source = r#"
fn run(traversal: &mut Traversal) {
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("&mut Traversal"));
}

#[test]
fn lifetime_reference_parameter_records_base_type() {
    let source = r#"
fn run<'a>(traversal: &'a mut Traversal) {
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "traversal", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Traversal");
    assert_eq!(declared(fact), Some("&'a mut Traversal"));
}

#[test]
fn pointer_parameter_records_base_type() {
    let source = r#"
unsafe fn run(config: *const Config) {
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, symbol(&symbols, "config", SymbolKind::Variable));
    assert_eq!(fact.resolved_type, "Config");
    assert_eq!(declared(fact), Some("*const Config"));
}

#[test]
fn boxed_dyn_parameter_records_container_base_type() {
    let source = r#"
fn run(handler: Box<dyn Handler>) {
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(
        &extractor,
        symbol(&symbols, "handler", SymbolKind::Variable),
    );
    assert_eq!(fact.resolved_type, "Box");
    assert_eq!(declared(fact), Some("Box<dyn Handler>"));
}

#[test]
fn tuple_typed_parameter_records_no_fact() {
    let source = r#"
fn run(pair: (u32, u32)) {
}
"#;
    let (symbols, extractor) = extract(source);
    let param = symbol(&symbols, "pair", SymbolKind::Variable);
    assert!(!extractor.base.type_info.contains_key(&param.id));
}

#[test]
fn self_parameter_records_impl_target_type() {
    let source = r#"
struct Traversal {
    depth: u32,
}

impl Traversal {
    fn step(&mut self, count: u32) {
        let next = count;
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "step", SymbolKind::Method);
    let self_param = symbol(&symbols, "self", SymbolKind::Variable);
    assert_eq!(self_param.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(role(self_param), Some("parameter"));
    assert_eq!(self_param.signature.as_deref(), Some("&mut self"));
    let fact = fact(&extractor, self_param);
    assert_eq!(fact.resolved_type, "Traversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn impl_method_parameters_and_locals_parent_to_the_method() {
    let source = r#"
struct Traversal {
    depth: u32,
}

impl Traversal {
    fn step(&mut self, count: u32) {
        let next: u32 = count + 1;
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "step", SymbolKind::Method);
    let count = symbol(&symbols, "count", SymbolKind::Variable);
    let next = symbol(&symbols, "next", SymbolKind::Variable);
    assert_eq!(count.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(next.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(role(count), Some("parameter"));
    assert_eq!(role(next), Some("local"));
    let next_fact = fact(&extractor, next);
    assert_eq!(next_fact.resolved_type, "u32");
    assert!(!next_fact.is_inferred);
}

#[test]
fn named_struct_field_records_declared_type() {
    let source = r#"
struct Config {
    retries: u32,
    traversal: graph::Traversal,
}
"#;
    let (symbols, extractor) = extract(source);
    let retries = symbol(&symbols, "retries", SymbolKind::Field);
    let retries_fact = fact(&extractor, retries);
    assert_eq!(retries_fact.resolved_type, "u32");
    assert!(!retries_fact.is_inferred);
    let traversal = symbol(&symbols, "traversal", SymbolKind::Field);
    let traversal_fact = fact(&extractor, traversal);
    assert_eq!(traversal_fact.resolved_type, "Traversal");
    assert_eq!(declared(traversal_fact), Some("graph::Traversal"));
}

#[test]
fn tuple_struct_positional_fields_record_nothing() {
    let source = r#"
struct Pair(u32, String);
"#;
    let (symbols, extractor) = extract(source);
    assert!(symbols.iter().all(|s| s.kind != SymbolKind::Field));
    assert!(extractor.base.type_info.is_empty());
}

#[test]
fn trait_default_method_self_parameter_gets_symbol_without_fact() {
    let source = r#"
trait Walker {
    fn walk(&self) {
        let steps = 1;
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "walk", SymbolKind::Function);
    let self_param = symbol(&symbols, "self", SymbolKind::Variable);
    assert_eq!(self_param.parent_id.as_deref(), Some(method.id.as_str()));
    assert!(!extractor.base.type_info.contains_key(&self_param.id));
    let local = symbol(&symbols, "steps", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(method.id.as_str()));
}

#[test]
fn function_type_parameters_do_not_become_symbols() {
    let source = r#"
fn run(callback: fn(inner: u32) -> u32) {
}
"#;
    let (symbols, _extractor) = extract(source);
    assert!(symbols.iter().all(|s| s.name != "inner"));
    assert!(symbols.iter().any(|s| s.name == "callback"));
}
