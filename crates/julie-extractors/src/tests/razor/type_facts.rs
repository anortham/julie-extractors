use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::razor::RazorExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, RazorExtractor) {
    extract_for_file("Counter.razor", source)
}

fn extract_for_file(file_path: &str, source: &str) -> (Vec<Symbol>, RazorExtractor) {
    let tree = init_parser(source, "razor");
    let mut extractor = RazorExtractor::new(
        "razor".to_string(),
        file_path.to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} symbol `{name}`"))
}

fn fact<'a>(extractor: &'a RazorExtractor, symbol: &Symbol) -> &'a TypeInfo {
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
}

fn no_fact(extractor: &RazorExtractor, symbol: &Symbol) {
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
        "expected no type fact for `{}`",
        symbol.name
    );
}

fn parameter<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| {
            s.name == name
                && s.kind == SymbolKind::Variable
                && s.metadata
                    .as_ref()
                    .and_then(|m| m.get("role"))
                    .is_some_and(|role| role == "parameter")
        })
        .unwrap_or_else(|| panic!("missing parameter `{name}`"))
}

fn code_block() -> &'static str {
    r#"
@page "/"

@code {
    private Widget _w;
    [Parameter] public string Title { get; set; }
    void Run(Widget w) {
        var x = new Widget();
        var y = Build();
        this.Refresh();
    }
}
"#
}

#[test]
fn code_field_is_kind_field_with_declared_widget_fact() {
    let (symbols, extractor) = extract(code_block());
    assert!(
        symbols
            .iter()
            .filter(|s| s.name == "_w")
            .all(|s| s.kind != SymbolKind::Variable)
    );
    let field = symbol(&symbols, "_w", SymbolKind::Field);
    let fact = fact(&extractor, field);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(!fact.is_inferred);
}

#[test]
fn parameter_property_records_string_fact() {
    let (symbols, extractor) = extract(code_block());
    let title = symbol(&symbols, "Title", SymbolKind::Property);
    let fact = fact(&extractor, title);
    assert_eq!(fact.resolved_type, "string");
    assert!(!fact.is_inferred);
}

#[test]
fn method_parameter_records_widget_fact() {
    let (symbols, extractor) = extract(code_block());
    let run = symbol(&symbols, "Run", SymbolKind::Method);
    let w = parameter(&symbols, "w");
    assert_eq!(w.parent_id.as_deref(), Some(run.id.as_str()));
    let fact = fact(&extractor, w);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(!fact.is_inferred);
}

#[test]
fn var_new_local_records_inferred_widget_fact() {
    let (symbols, extractor) = extract(code_block());
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    let fact = fact(&extractor, x);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(fact.is_inferred);
}

#[test]
fn var_call_local_records_symbol_without_fact() {
    let (symbols, extractor) = extract(code_block());
    let y = symbol(&symbols, "y", SymbolKind::Variable);
    no_fact(&extractor, y);
}

#[test]
fn this_call_records_component_name_as_receiver_type() {
    let (symbols, extractor) = extract(code_block());
    let _ = symbols;
    let refresh = extractor
        .base
        .identifiers
        .iter()
        .find(|id| id.name == "Refresh" && id.kind == IdentifierKind::Call)
        .expect("missing Refresh call identifier");
    assert_eq!(refresh.receiver_type.as_deref(), Some("Counter"));
}

#[test]
fn constructor_call_unknown_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
@page "/"

@code {
    void Run() {
        var unknown = Missing();
    }
}
"#,
    );
    let unknown = symbol(&symbols, "unknown", SymbolKind::Variable);
    no_fact(&extractor, unknown);
}

#[test]
fn constructor_call_imported_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
@page "/"

@code {
    void Run() {
        var imported = Other.Create();
    }
}
"#,
    );
    let imported = symbol(&symbols, "imported", SymbolKind::Variable);
    no_fact(&extractor, imported);
}

#[test]
fn constructor_call_non_constructor_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
@page "/"

@code {
    Widget Build() {
        return null;
    }
    void Run() {
        var built = Build();
    }
}
"#,
    );
    let built = symbol(&symbols, "built", SymbolKind::Variable);
    no_fact(&extractor, built);
}

#[test]
fn local_function_parameter_records_declared_fact() {
    let (symbols, extractor) = extract(
        r#"
@page "/"

@code {
    void Run() {
        void Inner(Widget w) {
        }
    }
}
"#,
    );
    let inner = symbol(&symbols, "Inner", SymbolKind::Method);
    let w = parameter(&symbols, "w");
    assert_eq!(w.parent_id.as_deref(), Some(inner.id.as_str()));
    let fact = fact(&extractor, w);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(!fact.is_inferred);
}

#[test]
fn local_function_records_return_type_fact() {
    let (symbols, extractor) = extract(
        r#"
@page "/"

@code {
    void Run() {
        Widget Inner() {
            return null;
        }
    }
}
"#,
    );
    let inner = symbol(&symbols, "Inner", SymbolKind::Method);
    let fact = fact(&extractor, inner);
    assert_eq!(fact.resolved_type, "Widget");
    assert!(!fact.is_inferred);
}

#[test]
fn no_code_field_row_is_kind_variable() {
    let (symbols, _) = extract(code_block());
    let field_rows: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get("type"))
                .is_some_and(|t| t == "field")
        })
        .collect();
    assert!(!field_rows.is_empty());
    assert!(field_rows.iter().all(|s| s.kind != SymbolKind::Variable));
}
