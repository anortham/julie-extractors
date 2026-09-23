use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility,
};
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

fn name_of(result: &ExtractionResults, id: Option<&str>) -> String {
    id.and_then(|id| result.symbols.iter().find(|symbol| symbol.id == id))
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn names_of_kind(result: &ExtractionResults, kind: SymbolKind) -> Vec<String> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == kind)
        .map(|symbol| symbol.name.clone())
        .collect()
}

fn resolved_calls(result: &ExtractionResults) -> Vec<(String, String)> {
    result
        .relationships
        .iter()
        .filter(|rel| rel.kind == RelationshipKind::Calls)
        .map(|rel| {
            (
                name_of(result, Some(&rel.from_symbol_id)),
                name_of(result, Some(&rel.to_symbol_id)),
            )
        })
        .collect()
}

fn pending_targets(result: &ExtractionResults) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .map(|pending| pending.target.display_name.clone())
        .collect()
}

fn call_names(result: &ExtractionResults) -> Vec<String> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| identifier.name.clone())
        .collect()
}

fn type_of(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

fn facts<'a>(
    result: &'a ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a std::collections::HashMap<String, serde_json::Value>> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .filter_map(|fact| fact.metadata.as_ref())
        .collect()
}

#[test]
fn nested_function_and_script_block_parameters_do_not_attach_to_the_outer_function() {
    let result = extract(
        "nested.ps1",
        r#"function Outer {
    param([string]$Path)
    function Inner {
        param([int]$Depth)
        $Depth
    }
    $sb = { param($Item) $Item }
    Invoke-Command -ScriptBlock { param($Session) $Session }
}
Describe 'widget' {
    It 'handles cases' -TestCases @(@{ N = 1 }) {
        param($N)
        $N
    }
}
"#,
    );
    let parent = |name: &str| {
        name_of(
            &result,
            symbol(&result, name, SymbolKind::Variable)
                .parent_id
                .as_deref(),
        )
    };
    assert_eq!(parent("Path"), "Outer");
    assert_eq!(parent("Depth"), "Inner");
    assert_eq!(parent("N"), "handles cases");
    let variables = names_of_kind(&result, SymbolKind::Variable);
    assert!(!variables.contains(&"Item".to_string()), "{variables:?}");
    assert!(!variables.contains(&"Session".to_string()), "{variables:?}");
}

#[test]
fn script_level_param_block_declares_file_scope_parameters() {
    let result = extract(
        "deploy.ps1",
        r#"<#
.SYNOPSIS
    Deploys the app.
.PARAMETER Environment
    The target environment.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Environment,
    [int]$Retries = 3
)
$Retries = 5
function Start-Deploy {
    Invoke-Step -Env $Environment -Retries $Retries
}
"#,
    );
    let environment = symbol(&result, "Environment", SymbolKind::Variable);
    assert_eq!(environment.parent_id, None);
    assert_eq!(type_of(&result, environment).as_deref(), Some("string"));
    assert_eq!(
        environment.doc_comment.as_deref(),
        Some("The target environment.")
    );
    assert_eq!(environment.annotations[0].annotation_key, "parameter");
    let retries: Vec<&Symbol> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "Retries")
        .collect();
    assert_eq!(retries.len(), 1, "a reassignment adds no second symbol");
    assert_eq!(type_of(&result, retries[0]).as_deref(), Some("int"));
}

#[test]
fn parameter_signature_ends_with_its_own_variable_and_takes_its_comment_as_doc() {
    let result = extract(
        "report.ps1",
        r#"function Send-Report {
    [CmdletBinding()]
    param(
        # The recipient.
        [Parameter(Mandatory)]
        [string] $To,
        [Parameter(Mandatory = $true, ValueFromPipeline = $true)]
        [string] $Body
    )
}
"#,
    );
    let to = symbol(&result, "To", SymbolKind::Variable);
    assert_eq!(
        to.signature.as_deref(),
        Some("[Parameter(Mandatory)] [string] $To")
    );
    assert_eq!(to.doc_comment.as_deref(), Some("# The recipient."));
    assert!(to.body_span.is_none());
    let body = symbol(&result, "Body", SymbolKind::Variable);
    assert_eq!(
        body.signature.as_deref(),
        Some("[Parameter(Mandatory = $true, ValueFromPipeline = $true)] [string] $Body")
    );
}

#[test]
fn comment_based_help_at_the_end_of_a_body_is_the_function_doc() {
    let result = extract(
        "help.ps1",
        r#"function Trailing-Help {
    param([string]$Name)
    $Name
    <#
    .SYNOPSIS
        Help placed at the end.
    #>
}
"#,
    );
    let doc = symbol(&result, "Trailing-Help", SymbolKind::Function)
        .doc_comment
        .clone()
        .unwrap_or_default();
    assert!(doc.contains("Help placed at the end."), "{doc:?}");
}

#[test]
fn call_operator_commands_emit_calls() {
    let result = extract(
        "build.ps1",
        r#"function Invoke-Compile { 1 }
function Invoke-Build {
    & Invoke-Compile -Release
    & "$PSScriptRoot\build.ps1" -Task Test
    & ./scripts/deploy.ps1
    & $exe --version
    . ./common.ps1
}
"#,
    );
    assert!(resolved_calls(&result).contains(&("Invoke-Build".into(), "Invoke-Compile".into())));
    let pending = pending_targets(&result);
    assert!(pending.contains(&"build.ps1".to_string()), "{pending:?}");
    assert!(pending.contains(&"deploy.ps1".to_string()), "{pending:?}");
    let calls = call_names(&result);
    assert!(
        !calls
            .iter()
            .any(|call| call.contains('$') || call == "common.ps1"),
        "{calls:?}"
    );
}

#[test]
fn command_calls_resolve_case_insensitively_and_through_aliases() {
    let result = extract(
        "case.ps1",
        r#"function Get-Config { 1 }
function Get-Thing { 2 }
Set-Alias -Name gt -Value Get-Thing
function Invoke-Deploy {
    get-config
    GET-CONFIG
    gt
}
describe 'lower' {
    it 'case' { Get-Foo }
    beforeall { Get-Foo }
}
"#,
    );
    let calls = resolved_calls(&result);
    let to_config = calls
        .iter()
        .filter(|call| *call == &("Invoke-Deploy".into(), "Get-Config".into()))
        .count();
    assert_eq!(to_config, 2, "{calls:?}");
    assert!(
        calls.contains(&("Invoke-Deploy".into(), "Get-Thing".into())),
        "{calls:?}"
    );
    assert!(!pending_targets(&result).contains(&"get-config".to_string()));
    let alias = symbol(&result, "gt", SymbolKind::Import);
    assert_eq!(
        alias.metadata.as_ref().unwrap()["aliasTarget"],
        serde_json::json!("Get-Thing")
    );
    let container = symbol(&result, "lower", SymbolKind::Function);
    assert_eq!(
        container.metadata.as_ref().unwrap()["test_container"],
        serde_json::json!(true)
    );
    assert!(result.symbols.iter().any(|s| s.name == "case"));
    assert!(result.symbols.iter().any(|s| s.name == "BeforeAll"));
}

#[test]
fn scoped_function_names_drop_the_scope_qualifier() {
    let result = extract(
        "scoped.ps1",
        r#"function global:Get-Tool { Resolve-Tool }
function script:Resolve-Tool($Name) { "tool:$Name" }
function private:Hidden-Tool { 1 }
"#,
    );
    let tool = symbol(&result, "Get-Tool", SymbolKind::Function);
    assert_eq!(
        tool.metadata.as_ref().unwrap()["scope"],
        serde_json::json!("global")
    );
    assert_eq!(
        symbol(&result, "Hidden-Tool", SymbolKind::Function).visibility,
        Some(Visibility::Private)
    );
    assert!(resolved_calls(&result).contains(&("Get-Tool".into(), "Resolve-Tool".into())));
}

#[test]
fn class_modifiers_come_from_member_keywords_not_member_text() {
    let result = extract(
        "panel.ps1",
        r#"class Panel {
    [bool]$hiddenFlag
    hidden [int]$Secret
    [void] Show() {
        $this.hiddenFlag = $false
    }
    static [Panel] Default() { return [Panel]::new() }
    Hidden [void] Conceal() { }
}
"#,
    );
    let flag = symbol(&result, "hiddenFlag", SymbolKind::Property);
    assert_eq!(flag.visibility, Some(Visibility::Public));
    assert_eq!(flag.signature.as_deref(), Some("[bool]$hiddenFlag"));
    assert_eq!(
        symbol(&result, "Secret", SymbolKind::Property).visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        symbol(&result, "Show", SymbolKind::Method).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        symbol(&result, "Conceal", SymbolKind::Method).visibility,
        Some(Visibility::Private)
    );
    let default = symbol(&result, "Default", SymbolKind::Method);
    assert_eq!(
        default.metadata.as_ref().unwrap()["isStatic"],
        serde_json::json!(true)
    );
}

#[test]
fn return_types_output_type_and_casts_record_type_facts() {
    let result = extract(
        "billing.ps1",
        r#"class Invoice { [decimal]$Total }
class Billing {
    [Invoice] Create() { return [Invoice]::new() }
    static [Invoice[]] All() { return @() }
    [void] Reset() { }
}
function Get-Invoice {
    [CmdletBinding()]
    [OutputType([Invoice])]
    param([string]$Id)
    $inv = [Invoice]$null
    $inv
}
"#,
    );
    let type_named = |name: &str, kind| type_of(&result, symbol(&result, name, kind));
    assert_eq!(
        type_named("Create", SymbolKind::Method).as_deref(),
        Some("Invoice")
    );
    assert_eq!(
        type_named("All", SymbolKind::Method).as_deref(),
        Some("Invoice[]")
    );
    assert_eq!(type_named("Reset", SymbolKind::Method), None);
    assert_eq!(
        type_named("Get-Invoice", SymbolKind::Function).as_deref(),
        Some("Invoice")
    );
    assert_eq!(
        type_named("inv", SymbolKind::Variable).as_deref(),
        Some("Invoice")
    );
}

#[test]
fn class_member_attributes_become_annotations() {
    let result = extract(
        "res.ps1",
        r#"class FileRes {
    [DscProperty(Key)]
    [string]$Path
    [ValidateNotNullOrEmpty()][string]$Ensure
}
"#,
    );
    let path = symbol(&result, "Path", SymbolKind::Property);
    assert_eq!(path.annotations[0].annotation_key, "dscproperty");
    assert!(path.body_span.is_none());
    let ensure = symbol(&result, "Ensure", SymbolKind::Property);
    assert_eq!(
        ensure.annotations[0].annotation_key,
        "validatenotnullorempty"
    );
}

#[test]
fn module_commands_emit_one_row_per_member_and_narrow_unexported_functions() {
    let result = extract(
        "Tools.psm1",
        r#"#Requires -Modules Az.Accounts, @{ ModuleName = 'Pester'; ModuleVersion = '5.0' }
using namespace System.Text
function Get-Public { 'pub' }
function Get-Internal { 'internal' }
Export-ModuleMember -Function 'Get-Public', 'Set-*' -Alias 'gp'
Export-ModuleMember -Variable Config
New-Alias gp Get-Public
Import-Module "$PSScriptRoot\Helpers.psm1" -Force
Import-Module ./Local/Tools.psm1
Import-Module -Name Az.Storage -MinimumVersion 2.0
Import-DscResource -ModuleName xWebAdministration
. (Join-Path $PSScriptRoot 'Common.ps1')
foreach ($f in $files) { . $f.FullName }
"#,
    );
    assert_eq!(
        names_of_kind(&result, SymbolKind::Export),
        ["Get-Public", "Set-*", "gp", "Config"]
    );
    let imports = names_of_kind(&result, SymbolKind::Import);
    for expected in [
        "Az.Accounts",
        "Pester",
        "System.Text",
        "gp",
        "Helpers",
        "Tools",
        "Az.Storage",
        "xWebAdministration",
        "Common",
    ] {
        assert!(
            imports.contains(&expected.to_string()),
            "{expected}: {imports:?}"
        );
    }
    assert!(
        !imports.iter().any(|name| name.contains('$') || name == "."),
        "{imports:?}"
    );
    assert_eq!(
        symbol(&result, "Get-Internal", SymbolKind::Function).visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        symbol(&result, "Get-Public", SymbolKind::Function).visibility,
        Some(Visibility::Public)
    );
    assert!(!call_names(&result).contains(&"using".to_string()));
    let using_import = symbol(&result, "System.Text", SymbolKind::Import);
    assert_eq!(using_import.doc_comment, None);
}

#[test]
fn http_cmdlets_emit_client_request_facts() {
    let result = extract(
        "client.ps1",
        r#"function Send-It {
    Invoke-RestMethod -Method Post -Uri 'https://api.contoso.com/orders' -Body '{}'
    irm https://api.contoso.com/health
    Invoke-WebRequest "https://api.contoso.com/orders/$Id"
}
"#,
    );
    let requests: Vec<(String, String)> = facts(&result, "http.client_request.v1")
        .into_iter()
        .map(|meta| {
            (
                meta["verb"].as_str().unwrap().to_string(),
                meta["target_path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        requests,
        [
            (
                "POST".to_string(),
                "https://api.contoso.com/orders".to_string()
            ),
            (
                "GET".to_string(),
                "https://api.contoso.com/health".to_string()
            ),
        ]
    );
}

#[test]
fn pipeline_facts_need_a_pipe_operator() {
    let result = extract(
        "pipe.ps1",
        r#"function Get-Names {
    $names = Get-Process | Select-Object -ExpandProperty Name
    Write-Host "a|b"
    $parts = $line -split '\|'
    Get-Item . | Out-Null
}
"#,
    );
    let lines: Vec<u32> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "powershell.pipeline_expression.v1")
        .map(|fact| fact.start_line)
        .collect();
    assert_eq!(lines, [2, 5]);
}

#[test]
fn dsc_resources_emit_facts_and_property_keys_are_not_calls() {
    let result = extract(
        "web.ps1",
        r#"Configuration WebServer {
    Import-DscResource -ModuleName PSDesiredStateConfiguration
    Node 'localhost' {
        WindowsFeature IIS {
            Ensure = 'Present'
            Name   = 'Web-Server'
        }
        File Site {
            DestinationPath = 'C:\inetpub\index.html'
            DependsOn       = '[WindowsFeature]IIS'
        }
    }
}
"#,
    );
    let resources = facts(&result, "powershell.dsc_resource.v1");
    assert_eq!(resources.len(), 2);
    assert_eq!(
        resources[0]["resource_type"],
        serde_json::json!("WindowsFeature")
    );
    assert_eq!(resources[0]["resource_name"], serde_json::json!("IIS"));
    assert_eq!(resources[0]["node_name"], serde_json::json!("localhost"));
    assert_eq!(
        resources[1]["depends_on"],
        serde_json::json!(["[WindowsFeature]IIS"])
    );
    assert!(
        names_of_kind(&result, SymbolKind::Import)
            .contains(&"PSDesiredStateConfiguration".to_string())
    );
    let calls = call_names(&result);
    for key in ["Ensure", "Name", "DestinationPath", "DependsOn"] {
        assert!(!calls.contains(&key.to_string()), "{key}: {calls:?}");
    }
}

#[test]
fn build_tasks_are_callers_of_the_functions_they_run() {
    let result = extract(
        "psakefile.ps1",
        r#"function NetCliBuild { 1 }
task default -depends Test,Package
task Build -depends Clean {
  NetCliBuild
}
"#,
    );
    let build = symbol(&result, "Build", SymbolKind::Function);
    assert_eq!(
        build.signature.as_deref(),
        Some("task Build -depends Clean")
    );
    assert_eq!(
        build.metadata.as_ref().unwrap()["depends"],
        serde_json::json!(["Clean"])
    );
    let default = symbol(&result, "default", SymbolKind::Function);
    assert_eq!(
        default.metadata.as_ref().unwrap()["depends"],
        serde_json::json!(["Test", "Package"])
    );
    assert!(resolved_calls(&result).contains(&("Build".into(), "NetCliBuild".into())));
}

#[test]
fn module_manifest_emits_imports_exports_and_key_facts() {
    let result = extract(
        "MyModule.psd1",
        r#"@{
    RootModule        = 'MyModule.psm1'
    ModuleVersion     = '1.2.0'
    RequiredModules   = @('Az.Accounts', @{ ModuleName = 'Pester'; ModuleVersion = '5.0.0' })
    FunctionsToExport = @('Get-Thing', 'Set-Thing')
    AliasesToExport   = @('gt')
    PrivateData = @{ PSData = @{ Tags = @('tools') } }
}
"#,
    );
    assert_eq!(
        names_of_kind(&result, SymbolKind::Import),
        ["MyModule", "Az.Accounts", "Pester"]
    );
    assert_eq!(
        names_of_kind(&result, SymbolKind::Export),
        ["Get-Thing", "Set-Thing", "gt"]
    );
    let manifest = facts(&result, "powershell.module_manifest.v1");
    assert_eq!(manifest.len(), 1);
    assert_eq!(manifest[0]["module_version"], serde_json::json!("1.2.0"));
    assert_eq!(
        manifest[0]["root_module"],
        serde_json::json!("MyModule.psm1")
    );
    let paths: Vec<&str> = facts(&result, "powershell.data_key.v1")
        .into_iter()
        .map(|meta| meta["key_path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"PrivateData.PSData.Tags"), "{paths:?}");
    assert!(paths.contains(&"RequiredModules.ModuleName"), "{paths:?}");
}
