use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).expect("PowerShell extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:#?}", result.symbols))
}

fn call_edges(result: &ExtractionResults) -> Vec<(String, String)> {
    let name_of = |id: &str| {
        result
            .symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| symbol.name.clone())
            .unwrap_or_default()
    };
    let mut edges: Vec<(String, String)> = result
        .relationships
        .iter()
        .filter(|rel| rel.kind == RelationshipKind::Calls)
        .map(|rel| (name_of(&rel.from_symbol_id), name_of(&rel.to_symbol_id)))
        .collect();
    edges.extend(
        result
            .structured_pending_relationships
            .iter()
            .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
            .map(|pending| {
                (
                    name_of(&pending.pending.from_symbol_id),
                    format!("pending:{}", pending.target.display_name),
                )
            }),
    );
    edges
}

fn has_edge(edges: &[(String, String)], from: &str, to: &str) -> bool {
    edges.iter().any(|(f, t)| f == from && t == to)
}

#[test]
fn commands_in_class_members_and_pester_blocks_emit_call_rows() {
    let result = extract(
        "callers.Tests.ps1",
        r#"function Invoke-Local { 1 }
class Svc {
    Svc() { Initialize-Svc; Invoke-Local }
    [void] Go() { Invoke-Remote -Name 'x'; Invoke-Local }
}
Describe 'Foo' -Tag 'Unit' {
    It 'bar' -Tag 'Slow' { Get-Foo; Invoke-Local }
    AfterAll { Remove-Foo }
}
"#,
    );
    let edges = call_edges(&result);

    assert!(has_edge(&edges, "Svc", "Invoke-Local"), "{edges:?}");
    assert!(
        has_edge(&edges, "Svc", "pending:Initialize-Svc"),
        "{edges:?}"
    );
    assert!(has_edge(&edges, "Go", "Invoke-Local"), "{edges:?}");
    assert!(has_edge(&edges, "Go", "pending:Invoke-Remote"), "{edges:?}");
    assert!(has_edge(&edges, "bar", "pending:Get-Foo"), "{edges:?}");
    assert!(has_edge(&edges, "bar", "Invoke-Local"), "{edges:?}");
    assert!(
        has_edge(&edges, "AfterAll", "pending:Remove-Foo"),
        "{edges:?}"
    );
    assert!(
        edges.iter().all(|(_, to)| !matches!(
            to.as_str(),
            "pending:It" | "pending:Describe" | "pending:AfterAll"
        )),
        "Pester DSL keywords are symbol declarations, not calls: {edges:?}"
    );
}

#[test]
fn pending_and_identifier_receivers_drop_the_variable_sigil() {
    let result = extract(
        "receiver.ps1",
        r#"class Cart {
    [void] Checkout() { }
}
function Complete-Order {
    param([Cart] $Cart)
    $Cart.Checkout()
    $other = [Cart]::new()
    $other.Checkout()
    $global:Config.Save()
}
"#,
    );
    let receivers: Vec<(String, Option<String>)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.display_name.clone(),
                pending.target.receiver.clone(),
            )
        })
        .collect();

    assert!(
        receivers.contains(&("Cart.Checkout".into(), Some("Cart".into()))),
        "{receivers:?}"
    );
    assert!(
        receivers.contains(&("other.Checkout".into(), Some("other".into()))),
        "{receivers:?}"
    );
    assert!(
        receivers.contains(&("Config.Save".into(), Some("Config".into()))),
        "{receivers:?}"
    );

    let checkout_receivers: Vec<_> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "Checkout")
        .map(|identifier| {
            identifier
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("receiver"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        checkout_receivers,
        vec![Some("Cart".to_string()), Some("other".to_string())]
    );

    let new_call = result
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "new")
        .expect("static new call");
    assert_eq!(
        new_call
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(|value| value.as_str()),
        Some("Cart")
    );
}

#[test]
fn type_literals_in_every_position_emit_type_usage_identifiers() {
    let result = extract(
        "typeuse.ps1",
        r#"class Widget { [string] $Name }
class Gadget : Widget { [Widget] $Inner }
function New-Widget {
    [OutputType([Widget])]
    param([Widget] $Template, [Gadget[]] $Many)
    $w = [Widget]::new()
    $g = New-Object -TypeName Gadget
    $c = [Widget]$Template
    if ($c -is [Gadget]) { }
    $d = $c -as [Widget]
    try { } catch [System.IO.IOException] { }
    return $w
}
"#,
    );
    let usages: Vec<(u32, String)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
        .map(|identifier| (identifier.start_line, identifier.name.clone()))
        .collect();

    for expected in [
        (2, "Widget"),
        (4, "Widget"),
        (5, "Widget"),
        (5, "Gadget"),
        (6, "Widget"),
        (7, "Gadget"),
        (8, "Widget"),
        (9, "Gadget"),
        (10, "Widget"),
        (11, "System.IO.IOException"),
    ] {
        assert!(
            usages.contains(&(expected.0, expected.1.to_string())),
            "missing type_usage {expected:?}: {usages:?}"
        );
    }
    assert!(
        usages.iter().all(|(_, name)| name != "string"),
        "builtin primitive types are not type usages: {usages:?}"
    );
}

#[test]
fn comments_attach_only_to_the_adjacent_symbol() {
    let result = extract(
        "docs.ps1",
        r#"# Copyright (c) Contoso. Licensed under the MIT License.

function Get-A { 'a' }

function Get-First {
    <#
    .SYNOPSIS
    Returns the first item.
    #>
    [CmdletBinding()]
    param([object[]] $Items)
    $Items[0]
}

# A shape base class.
class Shape {
    [int]$Size
    [void] Open() { }
}
"#,
    );
    let doc = |name: &str, kind| symbol(&result, name, kind).doc_comment.clone();

    assert_eq!(doc("Get-A", SymbolKind::Function), None);
    assert!(
        doc("Get-First", SymbolKind::Function)
            .is_some_and(|doc| doc.contains("Returns the first item.")),
        "in-body help must be the doc comment"
    );
    assert_eq!(
        doc("Shape", SymbolKind::Class).as_deref(),
        Some("# A shape base class.")
    );
    assert_eq!(doc("Size", SymbolKind::Property), None);
    assert_eq!(doc("Open", SymbolKind::Method), None);
    assert_eq!(doc("Items", SymbolKind::Variable), None);
}

#[test]
fn only_plain_assignment_targets_become_variable_symbols() {
    let result = extract(
        "vars.ps1",
        r#"class Node {
    [int]$X
    Node([int]$x) { $this.X = $x }
}
function Update-Config {
    param([hashtable] $Config)
    $MAX = 5
    $MAX = 6
    $Config.Timeout = 30
    $Config['Retries'] = 5
    [Net.ServicePointManager]::SecurityProtocol = 'Tls12'
    $env:BUILD_ID = '42'
    $script:Hits = 1
    Get-ChildItem $PSScriptRoot | ForEach-Object { $_.Name } | Where-Object { $_ -ne $MAX }
    Write-Host $env:PATH
}
"#,
    );
    let mut variables: Vec<&str> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Variable)
        .map(|symbol| symbol.name.as_str())
        .collect();
    variables.sort_unstable();

    assert_eq!(variables, vec!["BUILD_ID", "Config", "Hits", "MAX", "x"]);
    assert!(
        result
            .symbols
            .iter()
            .filter_map(|symbol| symbol.doc_comment.as_deref())
            .all(|doc| !doc.contains("Variable") && !doc.contains("parameter")),
        "synthetic doc strings are not doc comments"
    );
}

#[test]
fn class_bases_come_from_the_declaration_not_the_body() {
    let result = extract(
        "inherit.ps1",
        r#"class Shape {
    static [int]$Count = 0
    Shape() { [Shape]::Count++ }
}
class Circle : Shape { [double]$Radius }
class Repo : BaseRepo, IDisposable {
    [void] Dispose() { }
}
class Plain {
    [void] Go() { $script:Hits = 1 }
}
"#,
    );
    assert_eq!(
        symbol(&result, "Shape", SymbolKind::Class)
            .signature
            .as_deref(),
        Some("class Shape")
    );
    assert_eq!(
        symbol(&result, "Plain", SymbolKind::Class)
            .signature
            .as_deref(),
        Some("class Plain")
    );
    assert_eq!(
        symbol(&result, "Repo", SymbolKind::Class)
            .signature
            .as_deref(),
        Some("class Repo : BaseRepo, IDisposable")
    );

    let circle = symbol(&result, "Circle", SymbolKind::Class);
    let shape = symbol(&result, "Shape", SymbolKind::Class);
    assert!(
        result
            .relationships
            .iter()
            .any(|rel| rel.kind == RelationshipKind::Extends
                && rel.from_symbol_id == circle.id
                && rel.to_symbol_id == shape.id)
    );
    assert_eq!(
        result
            .relationships
            .iter()
            .filter(|rel| matches!(
                rel.kind,
                RelationshipKind::Extends | RelationshipKind::Implements
            ))
            .count(),
        1
    );

    let repo = symbol(&result, "Repo", SymbolKind::Class);
    let bases: Vec<(RelationshipKind, String)> = result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.from_symbol_id == repo.id)
        .map(|pending| {
            (
                pending.pending.kind.clone(),
                pending.target.display_name.clone(),
            )
        })
        .collect();
    assert_eq!(
        bases,
        vec![
            (RelationshipKind::Extends, "BaseRepo".to_string()),
            (RelationshipKind::Implements, "IDisposable".to_string()),
        ]
    );
    for base in ["Shape", "BaseRepo", "IDisposable"] {
        assert!(
            result
                .identifiers
                .iter()
                .any(|identifier| identifier.kind == IdentifierKind::TypeUsage
                    && identifier.name == base),
            "missing base type usage {base}"
        );
    }
}

#[test]
fn module_export_line_creates_no_phantom_functions() {
    let result = extract(
        "Settings.psm1",
        r#"function Get-Setting {
    [CmdletBinding()]
    param([string] $Name)
    $Name
}
function Set-Setting {
    [CmdletBinding()]
    param([string] $Name, [string] $Value)
    $Value
}
Export-ModuleMember -function Get-Setting, Set-Setting
"#,
    );
    let functions: Vec<(&str, Option<&str>)> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .map(|symbol| (symbol.name.as_str(), symbol.parent_id.as_deref()))
        .collect();
    assert_eq!(
        functions,
        vec![("Get-Setting", None), ("Set-Setting", None)]
    );
}

#[test]
fn reassignment_dedup_keeps_distinct_scopes_and_drives() {
    let result = extract(
        "scopes.ps1",
        r#"function Set-Names {
    $name = 'local'
    $local:name = 'same local'
    $env:name = 'environment'
    $script:name = 'script'
    $script:name = 'script again'
    $global:name = 'global'
}
"#,
    );
    let signatures: Vec<&str> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Variable)
        .filter_map(|symbol| symbol.signature.as_deref())
        .collect();

    assert_eq!(
        signatures,
        vec![
            "$name = 'local'",
            "$env:name = 'environment'",
            "$script:name = 'script'",
            "$global:name = 'global'",
        ]
    );
}
