use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::vbnet::VbNetExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, VbNetExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_vb_dotnet::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = VbNetExtractor::new(
        "vbnet".to_string(),
        "type_facts.vb".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_with_calls(source: &str) -> (Vec<Symbol>, VbNetExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_vb_dotnet::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = VbNetExtractor::new(
        "vbnet".to_string(),
        "type_facts.vb".to_string(),
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
    extractor: &'a VbNetExtractor,
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

fn no_fact(extractor: &VbNetExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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
fn sub_parameters_record_declared_base_names() {
    let source = r#"
Class Sample
    Sub F(ByVal a As Foo, ByRef b As List(Of Foo))
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "F", SymbolKind::Method);
    let a = symbol(&symbols, "a", SymbolKind::Variable);
    let b = symbol(&symbols, "b", SymbolKind::Variable);
    assert_eq!(a.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(b.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(role(a), Some("parameter"));
    assert_eq!(role(b), Some("parameter"));
    let a_fact = fact(&extractor, &symbols, "a", SymbolKind::Variable);
    assert_eq!(a_fact.resolved_type, "Foo");
    assert!(!a_fact.is_inferred);
    assert_eq!(declared(a_fact), None);
    let b_fact = fact(&extractor, &symbols, "b", SymbolKind::Variable);
    assert_eq!(b_fact.resolved_type, "List");
    assert!(!b_fact.is_inferred);
    assert_eq!(declared(b_fact), Some("List(Of Foo)"));
}

#[test]
fn constructor_parameter_records_declared_type() {
    let source = r#"
Class Sample
    Sub New(ByVal seed As Foo)
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let ctor = symbol(&symbols, "New", SymbolKind::Constructor);
    let seed = symbol(&symbols, "seed", SymbolKind::Variable);
    assert_eq!(seed.parent_id.as_deref(), Some(ctor.id.as_str()));
    assert_eq!(role(seed), Some("parameter"));
    let fact = fact(&extractor, &symbols, "seed", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
}

#[test]
fn dim_nullable_records_base_name_and_parents_to_method() {
    let source = r#"
Class Sample
    Sub Run()
        Dim x As Foo?
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "Run", SymbolKind::Method);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(x.parent_id.as_deref(), Some(method.id.as_str()));
    assert_ne!(role(x), Some("parameter"));
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Foo?"));
}

#[test]
fn dim_new_same_file_constructor_records_inferred_fact() {
    let source = r#"
Class Foo
End Class
Class Sample
    Sub Run()
        Dim x = New Foo()
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "Run", SymbolKind::Method);
    let x = symbol(&symbols, "x", SymbolKind::Variable);
    assert_eq!(x.parent_id.as_deref(), Some(method.id.as_str()));
    let fact = fact(&extractor, &symbols, "x", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(fact.is_inferred);
}

#[test]
fn dim_as_new_records_declared_type() {
    let source = r#"
Class Foo
End Class
Class Sample
    Sub Run()
        Dim y As New Foo()
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "y", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
}

#[test]
fn dim_non_constructor_call_records_symbol_without_fact() {
    let source = r#"
Class Sample
    Sub Run()
        Dim z = Build()
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "z", SymbolKind::Variable);
}

#[test]
fn dim_unknown_constructor_records_symbol_without_fact() {
    let source = r#"
Class Sample
    Sub Run()
        Dim missing = New Missing()
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "missing", SymbolKind::Variable);
}

#[test]
fn dim_qualified_constructor_records_symbol_without_fact() {
    let source = r#"
Class Sample
    Sub Run()
        Dim imported = New Other.Foo()
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "imported", SymbolKind::Variable);
}

#[test]
fn dim_in_constructor_parents_to_constructor() {
    let source = r#"
Class Foo
End Class
Class Sample
    Sub New()
        Dim local As Foo
    End Sub
End Class
"#;
    let (symbols, extractor) = extract(source);
    let ctor = symbol(&symbols, "New", SymbolKind::Constructor);
    let local = symbol(&symbols, "local", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(ctor.id.as_str()));
    let fact = fact(&extractor, &symbols, "local", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
}

#[test]
fn field_and_property_record_declared_facts() {
    let source = r#"
Class Sample
    Private Index As Dictionary(Of String, List(Of Integer))
    Public Property Graph As SymbolGraph
End Class
"#;
    let (symbols, extractor) = extract(source);
    let index = fact(&extractor, &symbols, "Index", SymbolKind::Field);
    assert_eq!(index.resolved_type, "Dictionary");
    assert!(!index.is_inferred);
    assert_eq!(
        declared(index),
        Some("Dictionary(Of String, List(Of Integer))")
    );
    let graph = fact(&extractor, &symbols, "Graph", SymbolKind::Property);
    assert_eq!(graph.resolved_type, "SymbolGraph");
    assert!(!graph.is_inferred);
    assert_eq!(declared(graph), None);
}

#[test]
fn nullable_field_records_base_type_name() {
    let source = r#"
Class Sample
    Private Seed As Foo?
End Class
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "Seed", SymbolKind::Field);
    assert_eq!(fact.resolved_type, "Foo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Foo?"));
}

#[test]
fn me_and_mybase_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"
Class ServiceBase
End Class
Class OrderService
    Inherits ServiceBase
    Sub Process(other As Worker)
        Me.Persist()
        MyBase.Restore()
        other.Fetch()
    End Sub
End Class
Class Solo
    Sub Run()
        MyBase.Absent()
    End Sub
End Class
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
    assert_eq!(
        call("Persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        call("Restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(call("Fetch").receiver_type, None);
    assert_eq!(call("Absent").receiver_type, None);

    let pending = |name: &str| {
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .find(|p| p.target.terminal_name == name)
            .unwrap_or_else(|| panic!("missing structured pending for {name}"))
    };
    assert_eq!(
        pending("Persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("Restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(pending("Fetch").receiver_type, None);
    assert_eq!(pending("Absent").receiver_type, None);
}
