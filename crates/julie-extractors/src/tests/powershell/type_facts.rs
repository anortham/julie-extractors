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

fn assert_no_stray_brackets(extractor: &PowerShellExtractor) {
    for info in extractor.base.type_info.values() {
        let resolved = &info.resolved_type;
        let without_array = resolved
            .strip_suffix(']')
            .and_then(|head| head.rfind('['))
            .filter(|open| {
                resolved[open + 1..resolved.len() - 1]
                    .chars()
                    .all(|c| c == ',')
            })
            .map_or(resolved.as_str(), |open| &resolved[..open]);
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
        Some(&serde_json::json!(
            "[System.Collections.Generic.List[string]]"
        ))
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
    assert!(!extractor.base.type_info.contains_key(&ctor.id));
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

#[test]
fn array_types_keep_array_suffix() {
    let (symbols, extractor) = extract(
        r#"
class Box {
    [void] Run([Foo[]]$fs) {}
}
function Use {
    [string[]]$xs = @()
    [int[,]]$grid = $null
}
"#,
    );

    let xs = variable(&symbols, "xs");
    let xs_fact = fact(&extractor, xs);
    assert_eq!(xs_fact.resolved_type, "string[]");
    assert!(!xs_fact.is_inferred);
    assert_eq!(
        declared_metadata(xs_fact),
        Some(&serde_json::json!("[string[]]"))
    );
    let grid = variable(&symbols, "grid");
    assert_eq!(fact(&extractor, grid).resolved_type, "int[,]");
    let fs = parameter_symbols(&symbols, "fs");
    assert_eq!(fs.len(), 1);
    assert_eq!(fact(&extractor, fs[0]).resolved_type, "Foo[]");
    assert_no_stray_brackets(&extractor);
}

#[test]
fn nested_generic_records_base_name_and_declared_text() {
    let (symbols, extractor) = extract(
        r#"
function Use {
    [Dictionary[string, List[int]]]$index = @{}
}
"#,
    );

    let index = variable(&symbols, "index");
    let index_fact = fact(&extractor, index);
    assert_eq!(index_fact.resolved_type, "Dictionary");
    assert_eq!(
        declared_metadata(index_fact),
        Some(&serde_json::json!("[Dictionary[string, List[int]]]"))
    );
    assert_no_stray_brackets(&extractor);
}

#[test]
fn identifier_inside_constructor_body_is_contained_by_constructor() {
    let (symbols, extractor) = extract_with_calls(
        r#"
class Worker {
    [int]$Id

    Worker([int]$id) {
        $this.Id = $id
    }
}
"#,
    );

    let ctor = symbols
        .iter()
        .find(|s| s.name == "Worker" && s.kind == SymbolKind::Constructor)
        .expect("missing constructor Worker");
    let body_identifiers: Vec<_> = extractor
        .base
        .identifiers
        .iter()
        .filter(|id| id.start_line == 6)
        .collect();
    assert!(!body_identifiers.is_empty());
    for identifier in body_identifiers {
        assert_eq!(
            identifier.containing_symbol_id.as_deref(),
            Some(ctor.id.as_str()),
            "identifier `{}` is not contained by the constructor",
            identifier.name
        );
    }
}

#[test]
fn artifact_types_prefer_recorded_facts_over_legacy_inference() {
    let code = r#"
class Widget {}
function Use {
    $w = [Widget]::new()
    $other = [Foo]::Build()
}
"#;
    let tree = init_parser(code, "powershell");
    let results = crate::factory::extract_symbols_and_relationships(
        &tree,
        "test.ps1",
        code,
        "powershell",
        &PathBuf::from("/tmp/test"),
    )
    .expect("extraction succeeds");

    let widget_rows: Vec<_> = results
        .types
        .values()
        .filter(|info| info.resolved_type.eq_ignore_ascii_case("Widget"))
        .collect();
    assert_eq!(widget_rows.len(), 1);
    assert_eq!(widget_rows[0].resolved_type, "Widget");
    let foo_rows: Vec<_> = results
        .types
        .values()
        .filter(|info| info.resolved_type.eq_ignore_ascii_case("Foo"))
        .collect();
    assert_eq!(foo_rows.len(), 1);
    assert_eq!(foo_rows[0].resolved_type, "Foo");
}

fn inferred(code: &str, name: &str) -> Option<TypeInfo> {
    let (symbols, extractor) = extract(code);
    let variable = variable(&symbols, name);
    extractor.base.type_info.get(&variable.id).cloned()
}

fn assert_inferred(code: &str, name: &str, expected: &str) {
    let fact = inferred(code, name).unwrap_or_else(|| panic!("missing type fact for `{name}`"));
    assert_eq!(fact.resolved_type, expected);
    assert!(fact.is_inferred);
}

fn assert_not_inferred(code: &str, name: &str) {
    assert!(
        inferred(code, name).is_none(),
        "expected no type fact for `{name}`"
    );
}

#[test]
fn output_type_function_call_records_inferred_fact() {
    assert_inferred(
        r#"
class Workspace {}
function Get-Workspace {
    [CmdletBinding()]
    [OutputType([Workspace])]
    param([string]$Name)
}
function Use {
    $w = get-workspace -Name x
}
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn output_type_function_call_through_call_operator_records_inferred_fact() {
    assert_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    param()
}
$w = & Get-Workspace
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn parenthesized_call_records_inferred_fact() {
    assert_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    param()
}
$w = (Get-Workspace)
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn scope_qualified_function_call_records_inferred_fact() {
    assert_inferred(
        r#"
function script:Get-Workspace {
    [OutputType([Workspace])]
    param()
}
$w = Get-Workspace
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn output_type_with_named_argument_records_inferred_fact() {
    assert_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace], ParameterSetName = 'ById')]
    param()
}
$w = Get-Workspace
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn repeated_output_type_with_one_type_records_inferred_fact() {
    assert_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    [OutputType([Workspace])]
    param()
}
$w = Get-Workspace
"#,
        "w",
        "Workspace",
    );
}

#[test]
fn output_type_generic_records_base_name_and_declared_text() {
    let fact = inferred(
        r#"
function Get-Names {
    [OutputType([System.Collections.Generic.List[string]])]
    param()
}
$names = Get-Names
"#,
        "names",
    )
    .expect("missing type fact for `names`");
    assert_eq!(fact.resolved_type, "System.Collections.Generic.List");
    assert_eq!(
        declared_metadata(&fact),
        Some(&serde_json::json!(
            "[System.Collections.Generic.List[string]]"
        ))
    );
    assert!(fact.is_inferred);
}

#[test]
fn this_method_call_records_inferred_fact() {
    assert_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
    [void] Use() {
        $loaded = $this.Load()
    }
}
"#,
        "loaded",
        "Workspace",
    );
}

#[test]
fn static_method_call_on_same_file_class_records_inferred_fact() {
    assert_inferred(
        r#"
class Workspace {
    static [Workspace] Create() { return [Workspace]::new() }
}
function Use {
    $created = [Workspace]::Create()
}
"#,
        "created",
        "Workspace",
    );
}

#[test]
fn function_without_output_type_records_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    param()
}
$w = Get-Workspace
"#,
        "w",
    );
}

#[test]
fn same_named_functions_that_disagree_record_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    param()
}
function Get-Workspace {
    [OutputType([Folder])]
    param()
}
$w = Get-Workspace
"#,
        "w",
    );
}

#[test]
fn output_type_with_two_types_records_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace], [Folder])]
    param()
}
$w = Get-Workspace
"#,
        "w",
    );
}

#[test]
fn output_type_attributes_that_disagree_record_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    [OutputType([Folder])]
    param()
}
$w = Get-Workspace
"#,
        "w",
    );
}

#[test]
fn output_type_string_records_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    [OutputType('Folder')]
    [OutputType([Workspace])]
    param()
}
$w = Get-Workspace
"#,
        "w",
    );
}

#[test]
fn piped_call_records_no_fact() {
    assert_not_inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    param()
}
$w = Get-Workspace | Select-Object -First 1
"#,
        "w",
    );
}

#[test]
fn method_chain_after_this_call_records_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
    [void] Use() {
        $copy = $this.Load().Clone()
    }
}
"#,
        "copy",
    );
}

#[test]
fn this_call_outside_class_records_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
}
function Use {
    $loaded = $this.Load()
}
"#,
        "loaded",
    );
}

#[test]
fn this_call_to_static_method_records_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    static [Workspace] Load() { return $null }
    [void] Use() {
        $loaded = $this.Load()
    }
}
"#,
        "loaded",
    );
}

#[test]
fn static_call_to_instance_method_records_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
}
$loaded = [Workspace]::Load()
"#,
        "loaded",
    );
}

#[test]
fn this_static_access_records_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
    [void] Use() {
        $loaded = $this::Load()
    }
}
"#,
        "loaded",
    );
}

#[test]
fn overloads_that_disagree_record_no_fact() {
    assert_not_inferred(
        r#"
class Workspace {
    [Workspace] Load() { return $this }
    [string] Load([string]$path) { return $path }
    [void] Use() {
        $loaded = $this.Load()
    }
}
"#,
        "loaded",
    );
}

#[test]
fn this_call_to_base_class_method_records_no_fact() {
    assert_not_inferred(
        r#"
class Base {
    [Base] Load() { return $this }
}
class Workspace : Base {
    [void] Use() {
        $loaded = $this.Load()
    }
}
"#,
        "loaded",
    );
}

#[test]
fn void_and_untyped_methods_record_no_fact() {
    let code = r#"
class Workspace {
    [void] Reset() {}
    Refresh() {}
    [void] Use() {
        $reset = $this.Reset()
        $refreshed = $this.Refresh()
    }
}
"#;
    assert_not_inferred(code, "reset");
    assert_not_inferred(code, "refreshed");
}

#[test]
fn static_call_on_class_from_another_file_records_no_fact() {
    assert_not_inferred(
        r#"
class Folder {
    static [Folder] Create() { return [Folder]::new() }
}
$created = [Workspace]::Create()
"#,
        "created",
    );
}

#[test]
fn written_type_wins_over_call_inference() {
    let fact = inferred(
        r#"
function Get-Workspace {
    [OutputType([Workspace])]
    param()
}
[Folder]$w = Get-Workspace
"#,
        "w",
    )
    .expect("missing type fact for `w`");
    assert_eq!(fact.resolved_type, "Folder");
    assert!(!fact.is_inferred);
}
