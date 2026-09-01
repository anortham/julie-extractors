use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::dart::DartExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, DartExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_dart::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "type_facts.dart".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_with_calls(source: &str) -> (Vec<Symbol>, DartExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_dart::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "type_facts.dart".to_string(),
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
    extractor: &'a DartExtractor,
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

fn declared(fact: &TypeInfo) -> Option<&str> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
}

fn no_fact(extractor: &DartExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        !extractor.base.type_info.contains_key(&symbol.id),
        "unexpected type fact for {name}"
    );
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
class Foo {}
void f(Foo x, List<Foo> xs) {}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    let xs = symbol(&symbols, "xs", SymbolKind::Variable);
    assert_eq!(role(x), Some("parameter"));
    assert_eq!(role(xs), Some("parameter"));
    assert_eq!(x.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(xs.parent_id.as_deref(), Some(function.id.as_str()));
    let x_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);
    assert_eq!(declared(x_fact), None);
    let xs_fact = fact(&extractor, &symbols, "xs", SymbolKind::Variable);
    assert_eq!(xs_fact.resolved_type, "List");
    assert!(!xs_fact.is_inferred);
    assert_eq!(declared(xs_fact), Some("List<Foo>"));
}

#[test]
fn declared_local_wins_over_same_file_constructor() {
    let source = r#"
class Foo {}
void run() {
  final Foo x = Foo();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let local = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn untyped_same_file_constructors_record_inferred_facts() {
    let source = r#"
class Foo {}
void run() {
  final x = Foo();
  var y = new Foo();
  final z = Foo.named();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    for name in ["x", "y", "z"] {
        let local = symbol(&symbols, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
        let fact = fact(&extractor, &symbols, name, SymbolKind::Variable);
        assert_eq!(fact.resolved_type, "Foo");
        assert!(fact.is_inferred);
    }
}

#[test]
fn unknown_qualified_and_non_constructor_initializers_record_no_fact() {
    let source = r#"
void run() {
  final a = Unknown();
  final b = http.Client();
  final c = build();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    for name in ["a", "b", "c"] {
        let local = symbol(&symbols, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
        no_fact(&extractor, &symbols, name, SymbolKind::Variable);
    }
}

#[test]
fn nullable_local_records_base_name() {
    let source = r#"
class Foo {}
void run() {
  Foo? x;
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Foo?"));
}

#[test]
fn field_records_declared_type() {
    let source = r#"
class Worker {
  final int id;
  Worker(this.id);
}
"#;
    let (symbols, extractor) = extract(source);
    let field = fact(&extractor, &symbols, "id", SymbolKind::Field);
    assert_eq!(field.resolved_type, "int");
    assert!(!field.is_inferred);
    let constructor = symbol(&symbols, "Worker", SymbolKind::Constructor);
    let initializing = symbols
        .iter()
        .find(|s| s.name == "id" && s.kind == SymbolKind::Variable && role(s) == Some("parameter"))
        .expect("missing this.id parameter");
    assert_eq!(
        initializing.parent_id.as_deref(),
        Some(constructor.id.as_str())
    );
    let initializing_fact = extractor
        .base
        .type_info
        .get(&initializing.id)
        .expect("missing this.id fact");
    assert_eq!(initializing_fact.resolved_type, "int");
    assert!(!initializing_fact.is_inferred);
}

#[test]
fn this_and_super_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"
class Job {}
class Worker extends Job {
  void process(Worker other) {
    this.persist();
    super.restore();
    other.fetch();
  }
}
class Solo {
  void run() {
    super.absent();
  }
}
"#;
    let (_symbols, extractor) = extract_with_calls(source);
    let call = |name: &str| {
        extractor
            .base
            .identifiers
            .iter()
            .find(|id| id.name == name && id.kind == IdentifierKind::Call)
            .unwrap_or_else(|| panic!("missing call identifier {name}"))
    };
    assert_eq!(call("persist").receiver_type.as_deref(), Some("Worker"));
    assert_eq!(call("restore").receiver_type.as_deref(), Some("Job"));
    assert_eq!(call("fetch").receiver_type, None);
    assert_eq!(call("absent").receiver_type, None);

    let pending = |name: &str| {
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .find(|p| p.target.terminal_name == name)
            .unwrap_or_else(|| panic!("missing structured pending for {name}"))
    };
    assert_eq!(pending("persist").receiver_type.as_deref(), Some("Worker"));
    assert_eq!(pending("restore").receiver_type.as_deref(), Some("Job"));
    assert_eq!(pending("fetch").receiver_type, None);
    assert_eq!(pending("absent").receiver_type, None);
}

#[test]
fn nested_local_function_is_a_symbol_and_parents_its_parameters_and_locals() {
    let source = r#"
class Foo {}
void outer() {
  void inner(int z) {
    final w = Foo();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let outer = symbol(&symbols, "outer", SymbolKind::Function);
    let inner = symbol(&symbols, "inner", SymbolKind::Function);
    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    let z = symbol(&symbols, "z", SymbolKind::Variable);
    assert_eq!(role(z), Some("parameter"));
    assert_eq!(z.parent_id.as_deref(), Some(inner.id.as_str()));
    let w = symbol(&symbols, "w", SymbolKind::Variable);
    assert_eq!(w.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_eq!(
        fact(&extractor, &symbols, "w", SymbolKind::Variable).resolved_type,
        "Foo"
    );
}

#[test]
fn function_and_record_types_record_no_fact() {
    let source = r#"
void run(void Function() cb, (int, String) pair) {
  void Function(int) handler = (int v) {};
}
"#;
    let (symbols, extractor) = extract(source);
    for name in ["cb", "pair", "handler"] {
        no_fact(&extractor, &symbols, name, SymbolKind::Variable);
    }
}

#[test]
fn generic_and_qualified_types_reduce_to_the_base_name() {
    let source = r#"
void run(Map<String, int> m, prefix.Foo p, List<int>? xs) {}
"#;
    let (symbols, extractor) = extract(source);
    let m = fact(&extractor, &symbols, "m", SymbolKind::Variable);
    assert_eq!(m.resolved_type, "Map");
    assert!(!m.is_inferred);
    assert_eq!(declared(m), Some("Map<String, int>"));
    let p = fact(&extractor, &symbols, "p", SymbolKind::Variable);
    assert_eq!(p.resolved_type, "prefix.Foo");
    assert_eq!(declared(p), None);
    let xs = fact(&extractor, &symbols, "xs", SymbolKind::Variable);
    assert_eq!(xs.resolved_type, "List");
    assert_eq!(declared(xs), Some("List<int>?"));
}

#[test]
fn artifact_types_never_carry_keyword_text_for_locals() {
    let source = r#"
class Worker {
  Future<int> load() async {
    return 1;
  }
}
void run() {
  final a = Unknown();
  final b = http.Client();
  final c = build();
  var total = 0;
  const limit = 3;
}
"#;
    let results = crate::pipeline::extract_canonical_at(
        "type_facts.dart",
        source,
        std::path::Path::new("/repo"),
        crate::ExtractionLevel::Full,
    )
    .unwrap();
    let keyword_rows: Vec<(String, String)> = results
        .types
        .values()
        .filter(|info| {
            matches!(
                info.resolved_type.as_str(),
                "final" | "var" | "const" | "async" | "late" | "static"
            )
        })
        .map(|info| (info.symbol_id.clone(), info.resolved_type.clone()))
        .collect();
    assert!(keyword_rows.is_empty(), "keyword rows: {keyword_rows:?}");
    for name in ["a", "b", "c", "total", "limit"] {
        let local = symbol(&results.symbols, name, SymbolKind::Variable);
        assert!(
            !results.types.contains_key(&local.id),
            "unexpected artifact type for {name}"
        );
    }
}

#[test]
fn artifact_types_prefer_declared_facts_over_legacy_rows() {
    let source = r#"
class Foo {}
class Holder {
  final Foo item = Foo();
}
"#;
    let results = crate::pipeline::extract_canonical_at(
        "type_facts.dart",
        source,
        std::path::Path::new("/repo"),
        crate::ExtractionLevel::Full,
    )
    .unwrap();
    let item = symbol(&results.symbols, "item", SymbolKind::Field);
    let info = results.types.get(&item.id).expect("missing field type");
    assert_eq!(info.resolved_type, "Foo");
    assert!(!info.is_inferred);
}

#[test]
fn constructor_declared_after_use_records_inferred_fact() {
    let source = r#"
void run() {
  final x = Foo();
  var y = Foo.named();
}
class Foo {
  Foo();
  Foo.named();
}
"#;
    let (symbols, extractor) = extract(source);
    for name in ["x", "y"] {
        let fact = fact(&extractor, &symbols, name, SymbolKind::Variable);
        assert_eq!(fact.resolved_type, "Foo");
        assert!(fact.is_inferred);
    }
}

#[test]
fn every_declarator_in_a_local_declaration_becomes_a_symbol() {
    let source = r#"
class Foo {}
void run() {
  int a = 1, b = 2;
  String s, t;
  final u = Foo(), v = Foo();
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    for (name, expected, inferred) in [
        ("a", "int", false),
        ("b", "int", false),
        ("s", "String", false),
        ("t", "String", false),
        ("u", "Foo", true),
        ("v", "Foo", true),
    ] {
        let local = symbol(&symbols, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
        assert_eq!(local.name, name);
        let fact = fact(&extractor, &symbols, name, SymbolKind::Variable);
        assert_eq!(fact.resolved_type, expected);
        assert_eq!(fact.is_inferred, inferred);
    }
}
