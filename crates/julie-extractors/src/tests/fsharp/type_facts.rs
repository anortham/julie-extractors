use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::fsharp::FSharpExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;
use tree_sitter::Tree;

fn parse(code: &str) -> (Tree, FSharpExtractor) {
    let tree = init_parser(code, "fsharp");
    let extractor = FSharpExtractor::new(
        "fsharp".to_string(),
        "type_facts.fs".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    (tree, extractor)
}

fn extract(code: &str) -> (Vec<Symbol>, FSharpExtractor) {
    let (tree, mut extractor) = parse(code);
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing symbol `{name}`"))
}

fn variable<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing variable symbol `{name}`"))
}

fn fact<'a>(extractor: &'a FSharpExtractor, symbol: &Symbol) -> &'a TypeInfo {
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
}

fn declared_metadata(fact: &TypeInfo) -> Option<&str> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
}

fn no_fact(extractor: &FSharpExtractor, symbol: &Symbol) {
    assert!(
        !extractor.base.type_info.contains_key(&symbol.id),
        "expected no type fact for `{}`",
        symbol.name
    );
}

fn parameter_symbols<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
    symbols
        .iter()
        .filter(|s| {
            s.name == name
                && s.metadata
                    .as_ref()
                    .and_then(|m| m.get("role"))
                    .map(|role| role == &serde_json::json!("parameter"))
                    .unwrap_or(false)
        })
        .collect()
}

fn assert_resolved_types_are_base_names(extractor: &FSharpExtractor) {
    for type_info in extractor.base.type_info.values() {
        assert!(
            !type_info.resolved_type.contains('<') && !type_info.resolved_type.contains(' '),
            "resolved_type `{}` must not contain < or whitespace",
            type_info.resolved_type
        );
    }
}

#[test]
fn typed_let_parameters_record_facts_and_untyped_has_symbol_without_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  type Foo() = class end

  let f (x: Foo) (xs: Foo list) y = y
"#,
    );

    let function = symbol(&symbols, "f");
    let x_params = parameter_symbols(&symbols, "x");
    assert_eq!(x_params.len(), 1);
    assert_eq!(x_params[0].kind, SymbolKind::Variable);
    assert_eq!(x_params[0].parent_id.as_deref(), Some(function.id.as_str()));
    let x_fact = fact(&extractor, x_params[0]);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);

    let xs_params = parameter_symbols(&symbols, "xs");
    assert_eq!(xs_params.len(), 1);
    assert_eq!(
        xs_params[0].parent_id.as_deref(),
        Some(function.id.as_str())
    );
    let xs_fact = fact(&extractor, xs_params[0]);
    assert_eq!(xs_fact.resolved_type, "list");
    assert!(!xs_fact.is_inferred);
    assert_eq!(declared_metadata(xs_fact), Some("Foo list"));

    let y_params = parameter_symbols(&symbols, "y");
    assert_eq!(y_params.len(), 1);
    assert_eq!(y_params[0].parent_id.as_deref(), Some(function.id.as_str()));
    no_fact(&extractor, y_params[0]);
    assert_resolved_types_are_base_names(&extractor);
}

#[test]
fn member_this_receiver_records_enclosing_type_on_identifier_and_pending() {
    let source = r#"
module Domain =
  type Bar() = class end
  type Widget() =
    member this.Helper() = 0
    member this.Run(a: Bar) = this.Helper()
"#;
    let (tree, mut extractor) = parse(source);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);

    let run = symbol(&symbols, "Run");
    let a_params = parameter_symbols(&symbols, "a");
    assert_eq!(a_params.len(), 1);
    assert_eq!(a_params[0].parent_id.as_deref(), Some(run.id.as_str()));
    let a_fact = fact(&extractor, a_params[0]);
    assert_eq!(a_fact.resolved_type, "Bar");
    assert!(!a_fact.is_inferred);

    let helper_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "Helper" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(helper_calls.len(), 1);
    assert_eq!(helper_calls[0].receiver_type.as_deref(), Some("Widget"));

    let helper_pending: Vec<_> = extractor
        .base
        .get_structured_pending_relationships()
        .into_iter()
        .filter(|pending| pending.target.terminal_name == "Helper")
        .collect();
    assert_eq!(helper_pending.len(), 1);
    assert_eq!(helper_pending[0].receiver_type.as_deref(), Some("Widget"));
}

#[test]
fn member_named_instance_receiver_records_enclosing_type() {
    let source = r#"
module Domain =
  type Widget() =
    member this.Helper() = 0
    member x.Go() = x.Helper()
"#;
    let (tree, mut extractor) = parse(source);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);

    let helper_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "Helper" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(helper_calls.len(), 1);
    assert_eq!(helper_calls[0].receiver_type.as_deref(), Some("Widget"));

    let helper_pending: Vec<_> = extractor
        .base
        .get_structured_pending_relationships()
        .into_iter()
        .filter(|pending| pending.target.terminal_name == "Helper")
        .collect();
    assert_eq!(helper_pending.len(), 1);
    assert_eq!(helper_pending[0].receiver_type.as_deref(), Some("Widget"));
}

#[test]
fn other_receiver_does_not_record_receiver_type() {
    let source = r#"
module Domain =
  type Widget() =
    member this.Helper() = 0
    member this.CallOther(other: Widget) = other.Helper()
"#;
    let (tree, mut extractor) = parse(source);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);

    let helper_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "Helper" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(helper_calls.len(), 1);
    assert_eq!(helper_calls[0].receiver_type, None);

    let helper_pending: Vec<_> = extractor
        .base
        .get_structured_pending_relationships()
        .into_iter()
        .filter(|pending| pending.target.terminal_name == "Helper")
        .collect();
    assert_eq!(helper_pending.len(), 1);
    assert_eq!(helper_pending[0].receiver_type, None);
}

#[test]
fn type_abbrev_emits_type_symbol() {
    let (symbols, _) = extract(
        r#"
module Domain =
  type Id = int
"#,
    );

    let id = symbol(&symbols, "Id");
    assert_eq!(id.kind, SymbolKind::Type);
    assert!(!symbols.iter().any(|s| s.name == "int"));
}

#[test]
fn literal_let_records_inferred_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  let n = 1
"#,
    );

    let n = variable(&symbols, "n");
    let n_fact = fact(&extractor, n);
    assert_eq!(n_fact.resolved_type, "int");
    assert!(n_fact.is_inferred);
}

#[test]
fn generic_declared_type_records_structural_base_name() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  let nested: Map<string, List<int>> = Map.empty
  let qualified: Foo.Bar = Unchecked.defaultof<_>
"#,
    );

    let nested = variable(&symbols, "nested");
    let nested_fact = fact(&extractor, nested);
    assert_eq!(nested_fact.resolved_type, "Map");
    assert!(!nested_fact.is_inferred);
    assert_eq!(
        declared_metadata(nested_fact),
        Some("Map<string, List<int>>")
    );

    let qualified = variable(&symbols, "qualified");
    let qualified_fact = fact(&extractor, qualified);
    assert_eq!(qualified_fact.resolved_type, "Foo.Bar");
    assert!(!qualified_fact.is_inferred);
    assert_resolved_types_are_base_names(&extractor);
}

#[test]
fn constructor_call_same_file_records_inferred_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  type Store() = class end

  let x = Store()
"#,
    );

    let x = variable(&symbols, "x");
    let x_fact = fact(&extractor, x);
    assert_eq!(x_fact.resolved_type, "Store");
    assert!(x_fact.is_inferred);
}

#[test]
fn constructor_call_unknown_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  let x = Unknown()
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn constructor_call_imported_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  let x = System.Random()
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn constructor_call_non_constructor_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  let helper () = 1
  let x = helper()
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn type_abbrev_to_named_type_emits_type_symbol_without_case() {
    let (symbols, _) = extract(
        r#"
module Domain =
  type Alias = Foo
"#,
    );

    let alias = symbol(&symbols, "Alias");
    assert_eq!(alias.kind, SymbolKind::Type);
    assert!(!symbols.iter().any(|s| s.name == "Foo"));
}

#[test]
fn single_case_union_with_bar_keeps_union_kind_and_case() {
    let (symbols, _) = extract(
        r#"
module Domain =
  type Flag = | On
"#,
    );

    let flag = symbol(&symbols, "Flag");
    assert_eq!(flag.kind, SymbolKind::Union);
    let on = symbol(&symbols, "On");
    assert_eq!(on.kind, SymbolKind::EnumMember);
    assert_eq!(on.parent_id.as_deref(), Some(flag.id.as_str()));
}
