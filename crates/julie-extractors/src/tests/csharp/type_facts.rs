use crate::base::{SymbolKind, TypeInfo};
use crate::csharp::CSharpExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<crate::base::Symbol>, CSharpExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = CSharpExtractor::new(
        "csharp".to_string(),
        "type_facts.cs".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn fact<'a>(
    extractor: &'a CSharpExtractor,
    symbols: &[crate::base::Symbol],
    name: &str,
    kind: SymbolKind,
) -> &'a TypeInfo {
    let symbol = symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"));
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

#[test]
fn explicit_local_records_declared_type_without_inference() {
    let source = r#"
public class Sample {
  public void Run() {
    GraphTraversal traversal = Create();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "traversal", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "GraphTraversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn var_new_local_records_generic_base_name_as_inferred() {
    let source = r#"
public class Sample {
  public void Run() {
    var lookup = new Dictionary<string, int>();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "lookup", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Dictionary");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), Some("Dictionary<string, int>"));
}

#[test]
fn nullable_field_records_base_type_name() {
    let source = r#"
public class Sample {
  private GraphTraversal? _traversal;
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "_traversal", SymbolKind::Field);
    assert_eq!(fact.resolved_type, "GraphTraversal");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("GraphTraversal?"));
}

#[test]
fn generic_field_keeps_full_declared_text() {
    let source = r#"
public class Sample {
  private readonly IReadOnlyDictionary<string, Symbol> _symbols = new Dictionary<string, Symbol>();
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "_symbols", SymbolKind::Field);
    assert_eq!(fact.resolved_type, "IReadOnlyDictionary");
    assert!(!fact.resolved_type.contains(char::is_whitespace));
    assert!(!fact.resolved_type.contains('<'));
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("IReadOnlyDictionary<string, Symbol>"));
}

#[test]
fn property_records_declared_type() {
    let source = r#"
public class Sample {
  public SymbolGraph Graph { get; set; }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "Graph", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "SymbolGraph");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn constant_records_declared_type() {
    let source = r#"
public class Sample {
  private const string DefaultName = "default";
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "DefaultName", SymbolKind::Constant);
    assert_eq!(fact.resolved_type, "string");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn generic_parameter_records_base_name() {
    let source = r#"
public class Sample {
  public void Run(Func<string, bool> predicate) {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "predicate", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Func");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Func<string, bool>"));
}

#[test]
fn var_without_new_records_no_fact() {
    let source = r#"
public class Sample {
  public void Run(IEnumerable<int> items) {
    var streamed = items.ToList();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let symbol = symbols
        .iter()
        .find(|s| s.name == "streamed" && s.kind == SymbolKind::Variable)
        .unwrap();
    assert!(extractor.base.type_info.get(&symbol.id).is_none());
}

#[test]
fn tuple_typed_local_records_no_fact() {
    let source = r#"
public class Sample {
  public void Run() {
    (int, string) pair = GetPair();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let symbol = symbols
        .iter()
        .find(|s| s.name == "pair" && s.kind == SymbolKind::Variable)
        .unwrap();
    assert!(extractor.base.type_info.get(&symbol.id).is_none());
}

#[test]
fn target_typed_new_local_records_declared_type() {
    let source = r#"
public class Sample {
  public void Run() {
    GraphTraversal traversal = new();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "traversal", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "GraphTraversal");
    assert!(!fact.is_inferred);
}
