use crate::base::{ExtractionResults, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("PowerShell extraction")
}

fn imports(result: &ExtractionResults) -> Vec<(String, Option<String>, Option<String>)> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            let text = |key: &str| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(key))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            (symbol.name.clone(), text("source"), text("form"))
        })
        .collect()
}

fn row(name: &str, source: Option<&str>, form: &str) -> (String, Option<String>, Option<String>) {
    (
        name.to_string(),
        source.map(str::to_string),
        Some(form.to_string()),
    )
}

#[test]
fn dot_sourcing_keeps_the_path_as_written() {
    let result = extract(
        "scripts/run.ps1",
        ". ./lib/Common.ps1\n. \"$PSScriptRoot/Helpers.ps1\"\n. $PSScriptRoot\\Win.ps1\n. '..\\shared\\Paths.ps1'\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row("Common", Some("./lib/Common.ps1"), "."),
            row("Helpers", Some("$PSScriptRoot/Helpers.ps1"), "."),
            row("Win", Some("$PSScriptRoot\\Win.ps1"), "."),
            row("Paths", Some("..\\shared\\Paths.ps1"), "."),
        ]
    );
}

#[test]
fn import_module_rows_carry_source_and_form() {
    let result = extract(
        "scripts/run.ps1",
        "Import-Module ./Modules/Tools.psm1\nimport-module \"$PSScriptRoot\\Mods\\Inner.psd1\"\nImport-Module -Name Pester\nImport-Module Az.Accounts,Az.Storage\nImport-DscResource -ModuleName PSDesiredStateConfiguration\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row("Tools", Some("./Modules/Tools.psm1"), "Import-Module"),
            row(
                "Inner",
                Some("$PSScriptRoot\\Mods\\Inner.psd1"),
                "Import-Module"
            ),
            row("Pester", Some("Pester"), "Import-Module"),
            row("Az.Accounts", Some("Az.Accounts"), "Import-Module"),
            row("Az.Storage", Some("Az.Storage"), "Import-Module"),
            row(
                "PSDesiredStateConfiguration",
                Some("PSDesiredStateConfiguration"),
                "Import-DscResource"
            ),
        ]
    );
}

#[test]
fn using_statements_carry_source_and_form() {
    let result = extract(
        "scripts/run.ps1",
        "using module ./Modules/Other.psm1\nusing module Name.Mod\nusing module My-Module\nusing module \"./Spaced Dir/Third.psm1\"\nusing module @{ ModuleName = 'PSReadLine'; ModuleVersion = '2.0' }\nusing namespace System.IO\nusing assembly ./lib/Tool.dll\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row(
                "./Modules/Other.psm1",
                Some("./Modules/Other.psm1"),
                "using module"
            ),
            row("Name.Mod", Some("Name.Mod"), "using module"),
            row("My-Module", Some("My-Module"), "using module"),
            row(
                "./Spaced Dir/Third.psm1",
                Some("./Spaced Dir/Third.psm1"),
                "using module"
            ),
            row("PSReadLine", Some("PSReadLine"), "using module"),
            row("System.IO", Some("System.IO"), "using namespace"),
        ]
    );
}

#[test]
fn requires_aliases_and_manifests_carry_form() {
    let script = extract(
        "scripts/run.ps1",
        "#Requires -Modules Az.Accounts, @{ ModuleName = 'Pester'; ModuleVersion = '5.0' }\nSet-Alias -Name gh2 -Value Get-Help\nNew-Alias ll Get-ChildItem\n",
    );
    assert_eq!(
        imports(&script),
        vec![
            row("Pester", Some("Pester"), "#Requires"),
            row("Az.Accounts", Some("Az.Accounts"), "#Requires"),
            row("gh2", None, "Set-Alias"),
            row("ll", None, "New-Alias"),
        ]
    );

    let manifest = extract(
        "Man/Man.psd1",
        "@{\n  RootModule = 'Man.psm1'\n  NestedModules = @('Sub/Nested.psm1')\n  RequiredModules = @('Pester')\n  FunctionsToExport = @('Get-X')\n}\n",
    );
    assert_eq!(
        imports(&manifest),
        vec![
            row("Man", Some("Man.psm1"), "RootModule"),
            row("Nested", Some("Sub/Nested.psm1"), "NestedModules"),
            row("Pester", Some("Pester"), "RequiredModules"),
        ]
    );
    let export = manifest
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Export)
        .expect("export row");
    assert_eq!(
        export.metadata.as_ref().map(|metadata| metadata.len()),
        Some(1)
    );
}
