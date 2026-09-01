use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::scala::ScalaExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, ScalaExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_scala::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = ScalaExtractor::new(
        "scala".to_string(),
        "type_facts.scala".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, Vec<Identifier>, ScalaExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_scala::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = ScalaExtractor::new(
        "scala".to_string(),
        "type_facts.scala".to_string(),
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
    extractor: &'a ScalaExtractor,
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

fn no_fact(extractor: &ScalaExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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
fn typed_parameters_record_declared_facts_under_the_function() {
    let source = r#"
def f(x: Foo, xs: List[Foo]): Unit = ()
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    let xs = symbol(&symbols, "xs", SymbolKind::Variable);
    assert_eq!(x.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(xs.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(role(x), Some("parameter"));
    assert_eq!(role(xs), Some("parameter"));
    let x_fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);
    assert_eq!(declared(x_fact), None);
    let xs_fact = fact(&extractor, &symbols, "xs", SymbolKind::Variable);
    assert_eq!(xs_fact.resolved_type, "List");
    assert!(!xs_fact.is_inferred);
    assert_eq!(declared(xs_fact), Some("List[Foo]"));
    assert!(symbols.iter().all(|symbol| {
        role(symbol) != Some("parameter")
            || symbol.parent_id.as_deref() != symbols
                .iter()
                .find(|candidate| candidate.kind == SymbolKind::Class)
                .map(|class| class.id.as_str())
    }));
}

#[test]
fn local_val_declared_type_is_variable_with_fact() {
    let source = r#"
def run(): Unit = {
  val x: Foo = null
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert!(
        symbols
            .iter()
            .all(|symbol| !(symbol.name == "x" && symbol.kind == SymbolKind::Constant))
    );
}

#[test]
fn local_val_new_records_inferred_fact() {
    let source = r#"
def run(): Unit = {
  val x = new Foo()
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(fact.is_inferred);
}

#[test]
fn same_file_constructor_call_records_inferred_fact() {
    let source = r#"
class Foo
def run(): Unit = {
  val x = Foo()
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(fact.is_inferred);
}

#[test]
fn unknown_qualified_and_non_constructor_calls_record_no_fact() {
    let source = r#"
def build(): Int = 1
def run(): Unit = {
  val a = Unknown()
  val b = scala.collection.mutable.ListBuffer()
  val c = build()
}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "a", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "b", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "c", SymbolKind::Variable);
}

#[test]
fn class_parameters_are_properties_with_declared_facts() {
    let source = r#"
case class P(a: Foo)
class Q(a: Foo)
class Keep {
  val named: Bar = null
  var mutable: Baz = null
}
"#;
    let (symbols, extractor) = extract(source);
    let p = symbol(&symbols, "P", SymbolKind::Class);
    let q = symbol(&symbols, "Q", SymbolKind::Class);
    let keep = symbol(&symbols, "Keep", SymbolKind::Class);
    let case_field = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "a"
                && symbol.kind == SymbolKind::Property
                && symbol.parent_id.as_deref() == Some(p.id.as_str())
        })
        .expect("missing case class property a");
    let class_field = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "a"
                && symbol.kind == SymbolKind::Property
                && symbol.parent_id.as_deref() == Some(q.id.as_str())
        })
        .expect("missing class property a");
    assert_eq!(role(case_field), None);
    assert_eq!(role(class_field), None);
    let case_fact = extractor
        .base
        .type_info
        .get(&case_field.id)
        .expect("missing type fact for P.a");
    let class_fact = extractor
        .base
        .type_info
        .get(&class_field.id)
        .expect("missing type fact for Q.a");
    assert_eq!(case_fact.resolved_type, "Foo");
    assert!(!case_fact.is_inferred);
    assert_eq!(class_fact.resolved_type, "Foo");
    assert!(!class_fact.is_inferred);
    let named = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "named"
                && symbol.kind == SymbolKind::Property
                && symbol.parent_id.as_deref() == Some(keep.id.as_str())
        })
        .expect("class val lost property kind");
    let mutable = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "mutable"
                && symbol.kind == SymbolKind::Variable
                && symbol.parent_id.as_deref() == Some(keep.id.as_str())
        })
        .expect("class var lost variable kind");
    assert_eq!(
        extractor
            .base
            .type_info
            .get(&named.id)
            .map(|fact| fact.resolved_type.as_str()),
        Some("Bar")
    );
    assert_eq!(
        extractor
            .base
            .type_info
            .get(&mutable.id)
            .map(|fact| fact.resolved_type.as_str()),
        Some("Baz")
    );
    assert!(symbols.iter().all(|symbol| {
        role(symbol) != Some("parameter")
            || !matches!(
                symbols.iter().find(|parent| {
                    parent.id.as_str() == symbol.parent_id.as_deref().unwrap_or("")
                }),
                Some(parent) if parent.kind == SymbolKind::Class
            )
    }));
}

#[test]
fn secondary_constructor_is_constructor_with_parameter_symbols() {
    let source = r#"
class Box(seed: Int) {
  def this(n: Foo) = this(0)
}
"#;
    let (symbols, extractor) = extract(source);
    let class = symbol(&symbols, "Box", SymbolKind::Class);
    let ctor = symbol(&symbols, "Box", SymbolKind::Constructor);
    assert_eq!(ctor.parent_id.as_deref(), Some(class.id.as_str()));
    let n = symbol(&symbols, "n", SymbolKind::Variable);
    assert_eq!(n.parent_id.as_deref(), Some(ctor.id.as_str()));
    assert_eq!(role(n), Some("parameter"));
    let n_fact = fact(&extractor, &symbols, "n", SymbolKind::Variable);
    assert_eq!(n_fact.resolved_type, "Foo");
    assert!(!n_fact.is_inferred);
    let seed = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "seed"
                && symbol.kind == SymbolKind::Property
                && symbol.parent_id.as_deref() == Some(class.id.as_str())
        })
        .expect("primary constructor param should be a class property");
    assert_eq!(role(seed), None);
}

#[test]
fn this_method_call_records_receiver_type_on_identifier_and_pending() {
    let source = r#"
class Widget {
  def ping(): Unit = {
    this.m()
    other.m()
  }
}
"#;
    let (_, identifiers, extractor) = extract_calls(source);
    let calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "m" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].receiver_type.as_deref(), Some("Widget"));
    assert_eq!(calls[1].receiver_type, None);
    let pending = extractor.get_structured_pending_relationships();
    let pending_for = |receiver: &str| {
        pending
            .iter()
            .find(|p| {
                p.target.terminal_name == "m" && p.target.receiver.as_deref() == Some(receiver)
            })
            .unwrap_or_else(|| panic!("missing pending m on {receiver}"))
    };
    assert_eq!(pending_for("this").receiver_type.as_deref(), Some("Widget"));
    assert_eq!(pending_for("other").receiver_type, None);
}
