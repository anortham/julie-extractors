use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::ruby::RubyExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, RubyExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RubyExtractor::new(
        "type_facts.rb".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, Vec<Identifier>, RubyExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RubyExtractor::new(
        "type_facts.rb".to_string(),
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
    extractor: &'a RubyExtractor,
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

fn no_fact(extractor: &RubyExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
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

fn parameter<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    let parameter = symbol(symbols, name, SymbolKind::Variable);
    assert_eq!(role(parameter), Some("parameter"));
    parameter
}

#[test]
fn method_parameters_become_symbols_without_facts() {
    let source = r#"
class Sample
  def run(a, b = 1, *rest, key:, &blk)
  end
end
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    for name in ["a", "b", "rest", "key", "blk"] {
        let parameter = parameter(&symbols, name);
        assert_eq!(parameter.parent_id.as_deref(), Some(method.id.as_str()));
        no_fact(&extractor, &symbols, name, SymbolKind::Variable);
    }
}

#[test]
fn initialize_and_singleton_method_parameters_parent_to_callable() {
    let source = r#"
class Sample
  def initialize(start)
  end

  def self.build(source)
  end
end
"#;
    let (symbols, extractor) = extract(source);
    let ctor = symbol(&symbols, "initialize", SymbolKind::Constructor);
    let singleton = symbol(&symbols, "build", SymbolKind::Method);
    let start = parameter(&symbols, "start");
    let source_param = parameter(&symbols, "source");
    assert_eq!(start.parent_id.as_deref(), Some(ctor.id.as_str()));
    assert_eq!(
        source_param.parent_id.as_deref(),
        Some(singleton.id.as_str())
    );
    no_fact(&extractor, &symbols, "start", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "source", SymbolKind::Variable);
}

#[test]
fn ivar_assignment_is_field_under_class_and_first_wins() {
    let source = r#"
class Widget
  def initialize
    @count = 0
  end

  def reset
    @count = 1
  end

  def peek
    @count
  end
end
"#;
    let (symbols, extractor) = extract(source);
    let class = symbol(&symbols, "Widget", SymbolKind::Class);
    let count = symbol(&symbols, "@count", SymbolKind::Field);
    assert_eq!(count.parent_id.as_deref(), Some(class.id.as_str()));
    assert_eq!(symbols.iter().filter(|s| s.name == "@count").count(), 1);
    assert!(
        !symbols
            .iter()
            .any(|s| s.name == "@count" && s.kind == SymbolKind::Variable)
    );
    no_fact(&extractor, &symbols, "@count", SymbolKind::Field);
}

#[test]
fn class_variable_assignment_is_field_under_class() {
    let source = r#"
class Widget
  @@total = 0

  def bump
    @@total = 1
  end
end
"#;
    let (symbols, _) = extract(source);
    let class = symbol(&symbols, "Widget", SymbolKind::Class);
    let total = symbol(&symbols, "@@total", SymbolKind::Field);
    assert_eq!(total.parent_id.as_deref(), Some(class.id.as_str()));
    assert_eq!(symbols.iter().filter(|s| s.name == "@@total").count(), 1);
}

#[test]
fn bare_ivar_read_creates_no_symbol() {
    let source = r#"
class Widget
  def peek
    @missing
  end
end
"#;
    let (symbols, _) = extract(source);
    assert!(symbols.iter().all(|s| s.name != "@missing"));
}

#[test]
fn same_file_new_initializer_records_inferred_fact() {
    let source = r#"
class Widget
end

class Sample
  def run
    w = Widget.new
  end
end
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let local = symbol(&symbols, "w", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(method.id.as_str()));
    let fact = fact(&extractor, &symbols, "w", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(fact.is_inferred);
}

#[test]
fn unknown_qualified_and_non_constructor_initializers_record_no_fact() {
    let source = r#"
class Sample
  def run
    u = Unknown.new
    n = Net::HTTP.new
    v = build
  end
end
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "u", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "n", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "v", SymbolKind::Variable);
}

#[test]
fn self_calls_record_enclosing_class_on_identifier_and_pending() {
    let source = r#"
class Widget
  def ping
    self.helper
    other.helper
  end
end
"#;
    let (_, identifiers, extractor) = extract_calls(source);
    let helpers: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "helper")
        .collect();
    assert_eq!(helpers.len(), 2);
    let self_helper = helpers
        .iter()
        .find(|id| id.receiver_type.as_deref() == Some("Widget"))
        .unwrap_or_else(|| panic!("missing self.helper receiver_type"));
    assert_ne!(self_helper.kind, IdentifierKind::VariableRef);
    assert!(helpers.iter().any(|id| id.receiver_type.is_none()));

    let pending = extractor.get_structured_pending_relationships();
    let pending_for = |receiver: &str| {
        pending
            .iter()
            .find(|p| {
                p.target.terminal_name == "helper" && p.target.receiver.as_deref() == Some(receiver)
            })
            .unwrap_or_else(|| panic!("missing pending helper on {receiver}"))
    };
    assert_eq!(pending_for("self").receiver_type.as_deref(), Some("Widget"));
    assert_eq!(pending_for("other").receiver_type, None);
}
