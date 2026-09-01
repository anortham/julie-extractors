use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::java::JavaExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, JavaExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaExtractor::new(
        "java".to_string(),
        "type_facts.java".to_string(),
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

fn fact<'a>(
    extractor: &'a JavaExtractor,
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

#[test]
fn declared_local_records_declared_type_without_inference() {
    let source = r#"
class Sample {
  void run() {
    Job current = fetch();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let local = symbol(&symbols, "current", SymbolKind::Variable);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    assert_eq!(local.parent_id.as_deref(), Some(method.id.as_str()));
    let fact = fact(&extractor, &symbols, "current", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Job");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn var_new_local_records_generic_base_name_as_inferred() {
    let source = r#"
class Sample {
  void run() {
    var lookup = new HashMap<String, Integer>();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "lookup", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "HashMap");
    assert!(fact.is_inferred);
    assert_eq!(declared(fact), Some("HashMap<String, Integer>"));
}

#[test]
fn var_without_new_records_no_fact() {
    let source = r#"
class Sample {
  void run(Iterable<Integer> items) {
    var streamed = items.iterator();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let local = symbol(&symbols, "streamed", SymbolKind::Variable);
    assert!(extractor.base.type_info.get(&local.id).is_none());
}

#[test]
fn multi_declarator_local_records_fact_per_declarator() {
    let source = r#"
class Sample {
  void run() {
    int first = 1, second = 2;
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let first = fact(&extractor, &symbols, "first", SymbolKind::Variable);
    assert_eq!(first.resolved_type, "int");
    assert!(!first.is_inferred);
    let second = fact(&extractor, &symbols, "second", SymbolKind::Variable);
    assert_eq!(second.resolved_type, "int");
}

#[test]
fn wildcard_bounded_generic_local_records_base_name() {
    let source = r#"
class Sample {
  void run() {
    List<? extends Job> jobs = fetch();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "jobs", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "List");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("List<? extends Job>"));
}

#[test]
fn array_local_records_full_array_text() {
    let source = r#"
class Sample {
  void run() {
    String[] names = fetch();
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "names", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "String[]");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn typed_parameter_becomes_symbol_with_declared_fact() {
    let source = r#"
class Sample {
  void run(Job job, int count) {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let job = symbol(&symbols, "job", SymbolKind::Variable);
    assert_eq!(job.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(job.signature.as_deref(), Some("Job job"));
    assert_eq!(
        job.metadata
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str()),
        Some("parameter")
    );
    let job_fact = fact(&extractor, &symbols, "job", SymbolKind::Variable);
    assert_eq!(job_fact.resolved_type, "Job");
    assert!(!job_fact.is_inferred);
    let count_fact = fact(&extractor, &symbols, "count", SymbolKind::Variable);
    assert_eq!(count_fact.resolved_type, "int");
}

#[test]
fn generic_parameter_records_base_name() {
    let source = r#"
class Sample {
  void run(Map<String, List<Integer>> index) {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "index", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Map");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Map<String, List<Integer>>"));
}

#[test]
fn constructor_parameter_becomes_symbol_with_fact() {
    let source = r#"
class Sample {
  Sample(Job seed) {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let ctor = symbol(&symbols, "Sample", SymbolKind::Constructor);
    let seed = symbol(&symbols, "seed", SymbolKind::Variable);
    assert_eq!(seed.parent_id.as_deref(), Some(ctor.id.as_str()));
    let fact = fact(&extractor, &symbols, "seed", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Job");
}

#[test]
fn spread_parameter_becomes_symbol_without_fact() {
    let source = r#"
class Sample {
  void log(String... parts) {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let parts = symbol(&symbols, "parts", SymbolKind::Variable);
    assert_eq!(
        parts
            .metadata
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str()),
        Some("parameter")
    );
    assert!(extractor.base.type_info.get(&parts.id).is_none());
}

#[test]
fn lambda_parameters_produce_no_symbols() {
    let source = r#"
class Sample {
  void run(List<Job> jobs) {
    jobs.forEach(item -> process(item));
  }
}
"#;
    let (symbols, _extractor) = extract(source);
    assert!(!symbols.iter().any(|s| s.name == "item"));
}

#[test]
fn catch_parameter_produces_no_symbol() {
    let source = r#"
class Sample {
  void run() {
    try {
      fetch();
    } catch (IllegalStateException | IllegalArgumentException failure) {
    }
  }
}
"#;
    let (symbols, _extractor) = extract(source);
    assert!(!symbols.iter().any(|s| s.name == "failure"));
}

#[test]
fn generic_field_records_base_name_with_declared_metadata() {
    let source = r#"
class Sample {
  private Map<String, List<Integer>> index;
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "index", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "Map");
    assert!(!fact.resolved_type.contains('<'));
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Map<String, List<Integer>>"));
}

#[test]
fn primitive_field_and_constant_record_declared_type() {
    let source = r#"
class Sample {
  private final int id = 7;
  private static final Object LOCK = new Object();
}
"#;
    let (symbols, extractor) = extract(source);
    let id = fact(&extractor, &symbols, "id", SymbolKind::Property);
    assert_eq!(id.resolved_type, "int");
    assert!(!id.is_inferred);
    assert_eq!(declared(id), None);
    let lock = fact(&extractor, &symbols, "LOCK", SymbolKind::Constant);
    assert_eq!(lock.resolved_type, "Object");
    assert!(!lock.is_inferred);
}

#[test]
fn multi_declarator_field_records_fact_per_declarator() {
    let source = r#"
class Sample {
  private int width = 1, height = 2;
}
"#;
    let (symbols, extractor) = extract(source);
    let width = fact(&extractor, &symbols, "width", SymbolKind::Property);
    assert_eq!(width.resolved_type, "int");
    let height = fact(&extractor, &symbols, "height", SymbolKind::Property);
    assert_eq!(height.resolved_type, "int");
}

#[test]
fn generic_method_return_records_base_name() {
    let source = r#"
class Sample {
  Map<String, List<Integer>> snapshot() {
    return null;
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "snapshot", SymbolKind::Method);
    assert_eq!(fact.resolved_type, "Map");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Map<String, List<Integer>>"));
}

#[test]
fn scoped_return_type_records_full_dotted_text() {
    let source = r#"
class Sample {
  java.util.Locale locale() {
    return null;
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "locale", SymbolKind::Method);
    assert_eq!(fact.resolved_type, "java.util.Locale");
    assert!(!fact.is_inferred);
}

#[test]
fn void_method_and_constructor_record_no_fact() {
    let source = r#"
class Sample {
  Sample() {
  }

  void reset() {
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let reset = symbol(&symbols, "reset", SymbolKind::Method);
    assert!(extractor.base.type_info.get(&reset.id).is_none());
    let ctor = symbol(&symbols, "Sample", SymbolKind::Constructor);
    assert!(extractor.base.type_info.get(&ctor.id).is_none());
}
