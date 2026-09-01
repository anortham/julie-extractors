use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::r::RExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, RExtractor) {
    let tree = init_parser(source, "r");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RExtractor::new(
        "r".to_string(),
        "test.R".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, Vec<Identifier>, RExtractor) {
    let tree = init_parser(source, "r");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RExtractor::new(
        "r".to_string(),
        "test.R".to_string(),
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
    extractor: &'a RExtractor,
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

fn no_fact(extractor: &RExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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
fn namespaced_r6class_emits_class_parameter_and_self_receiver() {
    let source = r#"
Worker <- R6::R6Class("Worker", public = list(run = function(n) self$log(n)))
"#;
    let (symbols, identifiers, extractor) = extract_calls(source);
    let class = symbol(&symbols, "Worker", SymbolKind::Class);
    let run = symbols
        .iter()
        .find(|s| s.name == "run" && s.parent_id.as_deref() == Some(class.id.as_str()))
        .unwrap_or_else(|| panic!("missing run under Worker"));
    let n = symbol(&symbols, "n", SymbolKind::Variable);
    assert_eq!(role(n), Some("parameter"));
    assert_eq!(n.parent_id.as_deref(), Some(run.id.as_str()));
    no_fact(&extractor, &symbols, "n", SymbolKind::Variable);

    let logs: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "log" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].receiver_type.as_deref(), Some("Worker"));
    let pending = extractor.get_structured_pending_relationships();
    let log_pending = pending
        .iter()
        .find(|p| p.target.terminal_name == "log" && p.target.receiver.as_deref() == Some("self"))
        .unwrap_or_else(|| panic!("missing pending log on self"));
    assert_eq!(log_pending.receiver_type.as_deref(), Some("Worker"));
}

#[test]
fn r6_new_initializer_records_inferred_fact() {
    let source = r#"
Worker <- R6::R6Class("Worker", public = list(run = function() NULL))
w <- Worker$new()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "w", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Worker");
    assert!(fact.is_inferred);
}

#[test]
fn s4_new_string_initializer_records_inferred_fact() {
    let source = r#"
setClass("Point", slots = c(x = "numeric"))
p <- new("Point")
"#;
    let (symbols, extractor) = extract(source);
    let _class = symbol(&symbols, "Point", SymbolKind::Class);
    let fact = fact(&extractor, &symbols, "p", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Point");
    assert!(fact.is_inferred);
}

#[test]
fn same_file_class_call_records_inferred_fact() {
    let source = r#"
setClass("Foo")
x <- Foo()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(fact.is_inferred);
}

#[test]
fn unknown_constructor_records_symbol_without_fact() {
    let source = r#"
u <- Unknown$new()
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "u", SymbolKind::Variable);
}

#[test]
fn namespaced_constructor_records_symbol_without_fact() {
    let source = r#"
Worker <- R6::R6Class("Worker", public = list(run = function() NULL))
q <- pkg::Worker$new()
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "q", SymbolKind::Variable);
}

#[test]
fn non_constructor_calls_record_symbols_without_facts() {
    let source = r#"
d <- data.frame()
f <- fit(x)
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "d", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "f", SymbolKind::Variable);
}

#[test]
fn class_declared_after_use_still_records_inferred_fact() {
    let source = r#"
x <- Foo()
w <- Worker$new()
setClass("Foo")
Worker <- R6::R6Class("Worker", public = list(run = function() NULL))
"#;
    let (symbols, extractor) = extract(source);
    let x = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(x.resolved_type, "Foo");
    assert!(x.is_inferred);
    let w = fact(&extractor, &symbols, "w", SymbolKind::Variable);
    assert_eq!(w.resolved_type, "Worker");
    assert!(w.is_inferred);
}

#[test]
fn r6_method_symbols_span_only_their_own_definitions() {
    let source = r#"
Worker <- R6::R6Class(
  "Worker",
  public = list(
    id = NULL,
    initialize = function(id) {
      self$id <- id
    },
    run = function() {
      helper(self$id)
    }
  )
)
"#;
    let (symbols, identifiers, _extractor) = extract_calls(source);
    let initialize = symbol(&symbols, "initialize", SymbolKind::Method);
    let run = symbol(&symbols, "run", SymbolKind::Method);
    assert_eq!((initialize.start_line, initialize.end_line), (6, 8));
    assert_eq!((run.start_line, run.end_line), (9, 11));
    assert_eq!(
        initialize.signature.as_deref(),
        Some("initialize = function(id)")
    );
    assert_eq!(run.signature.as_deref(), Some("run = function()"));

    let helper = identifiers
        .iter()
        .find(|id| id.name == "helper" && id.kind == IdentifierKind::Call)
        .unwrap_or_else(|| panic!("missing helper call"));
    assert_eq!(
        helper.containing_symbol_id.as_deref(),
        Some(run.id.as_str())
    );
}
