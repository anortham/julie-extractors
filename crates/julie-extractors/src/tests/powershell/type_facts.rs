use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::powershell::PowerShellExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(code: &str) -> (Vec<Symbol>, PowerShellExtractor) {
    let tree = init_parser(code, "powershell");
    let mut extractor = PowerShellExtractor::new(
        "powershell".to_string(),
        "test.ps1".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_with_calls(code: &str) -> (Vec<Symbol>, PowerShellExtractor) {
    let tree = init_parser(code, "powershell");
    let mut extractor = PowerShellExtractor::new(
        "powershell".to_string(),
        "test.ps1".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);
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

fn fact<'a>(extractor: &'a PowerShellExtractor, symbol: &Symbol) -> &'a TypeInfo {
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
}

fn declared_metadata(fact: &TypeInfo) -> Option<&serde_json::Value> {
    fact.metadata.as_ref().and_then(|m| m.get("declared"))
}

fn no_fact(extractor: &PowerShellExtractor, symbol: &Symbol) {
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
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

fn assert_no_stray_brackets(extractor: &PowerShellExtractor) {
    for info in extractor.base.type_info.values() {
        let resolved = &info.resolved_type;
        let without_array = resolved.strip_suffix("[]").unwrap_or(resolved);
        assert!(
            !without_array.contains('[') && !without_array.contains(']'),
            "resolved_type `{}` keeps a non-array bracket",
            resolved
        );
    }
}

#[test]
fn advanced_function_parameter_records_declared_string_fact() {
    let (symbols, extractor) = extract(
        r#"
function Get-Name {
    [CmdletBinding()]
    param(
        [Parameter()]
        [string]
        $Name
    )
}
"#,
    );

    let get_name = symbol(&symbols, "Get-Name");
    let params = parameter_symbols(&symbols, "Name");
    assert_eq!(params.len(), 1);
    let param = params[0];
    assert_eq!(param.kind, SymbolKind::Variable);
    assert_eq!(param.parent_id.as_deref(), Some(get_name.id.as_str()));
    let param_fact = fact(&extractor, param);
    assert_eq!(param_fact.resolved_type, "string");
    assert!(!param_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn class_method_parameter_becomes_symbol_with_foo_fact() {
    let (symbols, extractor) = extract(
        r#"
class Box {
    [void] Run([Foo]$f) {}
}
"#,
    );

    let run = symbols
        .iter()
        .find(|s| s.name == "Run" && s.kind == SymbolKind::Method)
        .expect("missing method Run");
    let params = parameter_symbols(&symbols, "f");
    assert_eq!(params.len(), 1);
    let param = params[0];
    assert_eq!(param.kind, SymbolKind::Variable);
    assert_eq!(param.parent_id.as_deref(), Some(run.id.as_str()));
    let param_fact = fact(&extractor, param);
    assert_eq!(param_fact.resolved_type, "Foo");
    assert!(!param_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn generic_list_local_records_base_name_and_declared_text() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    [System.Collections.Generic.List[string]]$items = @()
}
"#,
    );

    let use_fn = symbol(&symbols, "Use");
    let items = variable(&symbols, "items");
    assert_eq!(items.parent_id.as_deref(), Some(use_fn.id.as_str()));
    let items_fact = fact(&extractor, items);
    assert_eq!(items_fact.resolved_type, "System.Collections.Generic.List");
    assert!(!items_fact.is_inferred);
    assert_eq!(
        declared_metadata(items_fact),
        Some(&serde_json::json!("[System.Collections.Generic.List[string]]"))
    );
    assert_no_stray_brackets(&extractor);
}

#[test]
fn typed_local_records_declared_fact() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    [Foo]$x = $null
}
"#,
    );

    let x = variable(&symbols, "x");
    let x_fact = fact(&extractor, x);
    assert_eq!(x_fact.resolved_type, "Foo");
    assert!(!x_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn new_expression_same_file_records_inferred_fact() {
    let (symbols, extractor) = extract(
        r#"
class Widget {}
function Use {
    $w = [Widget]::new()
}
"#,
    );

    let use_fn = symbol(&symbols, "Use");
    let w = variable(&symbols, "w");
    assert_eq!(w.parent_id.as_deref(), Some(use_fn.id.as_str()));
    let w_fact = fact(&extractor, w);
    assert_eq!(w_fact.resolved_type, "Widget");
    assert!(w_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn new_object_same_file_records_inferred_fact() {
    let (symbols, extractor) = extract(
        r#"
class Widget {}
function Use {
    $n = New-Object Widget
}
"#,
    );

    let n = variable(&symbols, "n");
    let n_fact = fact(&extractor, n);
    assert_eq!(n_fact.resolved_type, "Widget");
    assert!(n_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn command_assignment_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    $g = Get-Thing
}
"#,
    );

    let g = variable(&symbols, "g");
    no_fact(&extractor, g);
}

#[test]
fn constructor_call_unknown_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    $x = [Missing]::new()
}
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn constructor_call_qualified_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    $x = [Ns.Widget]::new()
}
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn constructor_call_non_constructor_records_no_fact() {
    let (symbols, extractor) = extract(
        r#"
class Widget {}
function Use {
    $x = Widget
}
"#,
    );

    let x = variable(&symbols, "x");
    no_fact(&extractor, x);
}

#[test]
fn class_constructor_uses_constructor_kind() {
    let (symbols, extractor) = extract(
        r#"
class Widget {
    Widget() {}
}
"#,
    );

    let ctor = symbols
        .iter()
        .find(|s| s.name == "Widget" && s.kind == SymbolKind::Constructor)
        .expect("missing constructor Widget");
    let class = symbols
        .iter()
        .find(|s| s.name == "Widget" && s.kind == SymbolKind::Class)
        .expect("missing class Widget");
    assert_eq!(ctor.parent_id.as_deref(), Some(class.id.as_str()));
    assert!(extractor.base.type_info.get(&ctor.id).is_none());
}

#[test]
fn class_property_records_declared_fact() {
    let (symbols, extractor) = extract(
        r#"
class Widget {
    [string]$Title
}
"#,
    );

    let title = symbols
        .iter()
        .find(|s| s.name == "Title" && s.kind == SymbolKind::Property)
        .expect("missing property Title");
    let title_fact = fact(&extractor, title);
    assert_eq!(title_fact.resolved_type, "string");
    assert!(!title_fact.is_inferred);
    assert_no_stray_brackets(&extractor);
}

#[test]
fn this_call_records_receiver_type_on_identifier_and_pending() {
    let (_symbols, extractor) = extract_with_calls(
        r#"
class Widget {
    [void] Run() {
        $this.Run()
        $other.Run()
    }
}
"#,
    );

    let run_calls: Vec<_> = extractor
        .base
        .identifiers
        .iter()
        .filter(|id| id.name == "Run" && id.kind == IdentifierKind::Call)
        .collect();
    assert_eq!(run_calls.len(), 2);
    assert_eq!(
        run_calls
            .iter()
            .filter(|id| id.receiver_type.as_deref() == Some("Widget"))
            .count(),
        1
    );
    assert_eq!(
        run_calls
            .iter()
            .filter(|id| id.receiver_type.is_none())
            .count(),
        1
    );

    let run_pending: Vec<_> = extractor
        .get_structured_pending_relationships()
        .into_iter()
        .filter(|pending| pending.target.terminal_name == "Run")
        .collect();
    assert_eq!(run_pending.len(), 2);
    assert_eq!(
        run_pending
            .iter()
            .filter(|pending| pending.receiver_type.as_deref() == Some("Widget"))
            .count(),
        1
    );
    assert_eq!(
        run_pending
            .iter()
            .filter(|pending| pending.receiver_type.is_none())
            .count(),
        1
    );
}
