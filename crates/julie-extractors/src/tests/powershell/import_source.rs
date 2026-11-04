use crate::base::{ExtractionResults, IdentifierKind, SymbolKind};
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
    let import_rows = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .collect::<Vec<_>>();
    assert_eq!(import_rows.len(), 6);
    assert!(import_rows.iter().all(|symbol| symbol.body_span.is_none()));
    assert!(import_rows.iter().all(|symbol| symbol.body_hash.is_none()));
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

#[test]
fn requires_module_option_hashtables_are_not_import_bodies() {
    let result = extract(
        "Tools.psm1",
        "#Requires -Modules Az.Accounts, @{ ModuleName = 'Pester'; ModuleVersion = '5.0' }\n",
    );
    let required = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .collect::<Vec<_>>();

    assert_eq!(
        imports(&result),
        vec![
            row("Pester", Some("Pester"), "#Requires"),
            row("Az.Accounts", Some("Az.Accounts"), "#Requires"),
        ]
    );
    assert_eq!(required.len(), 2);
    assert!(required.iter().all(|symbol| symbol.body_span.is_none()));
    assert!(required.iter().all(|symbol| symbol.body_hash.is_none()));
}

#[test]
fn variable_bodies_only_cover_script_block_values() {
    let source = r#"$hashtable = @{ Callback = { param($Item) $Item } }
$array = @()
$constructor = [Widget]::new()
$path = (Resolve-Path -Path 'input.txt').Path
$call = Invoke-Script { param($Item) $Item }
$scriptBlock = { param($Item) $Item }
$parenthesizedScriptBlock = ({ param($Item) $Item })
"#;
    let result = extract("scripts/run.ps1", source);

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    let variable = |name: &str| {
        result
            .symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Variable && symbol.name == name)
            .unwrap_or_else(|| panic!("missing variable {name}"))
    };
    for name in ["hashtable", "array", "constructor", "path", "call"] {
        let symbol = variable(name);
        assert!(symbol.body_span.is_none(), "{name}: {:?}", symbol.body_span);
        assert!(symbol.body_hash.is_none(), "{name}: {:?}", symbol.body_hash);
    }
    for name in ["scriptBlock", "parenthesizedScriptBlock"] {
        let symbol = variable(name);
        let span = symbol.body_span.as_ref().expect("script block body");
        assert_eq!(
            &source[span.start_byte as usize..span.end_byte as usize],
            "{ param($Item) $Item }"
        );
        assert_eq!(
            symbol.body_hash.as_deref(),
            Some("874a62554bc8176461b422b44b0ecae1")
        );
    }
}

#[test]
fn unquoted_resolve_path_arguments_accept_script_root_paths() {
    let source = r#"$cgManifestPath = (Resolve-Path -Path $PSScriptRoot\..\tools\cgmanifest.json).ProviderPath
"#;
    let mut parser = crate::pipeline::configured_parser_for_language("powershell").unwrap();
    let tree = parser.parse(source, None).unwrap();

    assert!(
        !tree.root_node().has_error(),
        "unexpected syntax nodes: {}",
        tree.root_node().to_sexp()
    );
}

#[test]
fn variable_prefix_paths_keep_source_and_variable_usage() {
    let source = r#"Import-Module $PSScriptRoot\Mods\Positional.psm1 -Force
Import-Module $PSScriptRoot/Mods/ForwardSlash.psm1; Import-Module -Name $PSScriptRoot\Mods\Named.psm1
Import-Module $PSScriptRoot\Mods/Piped.psm1 | Out-Null
Import-Module ($PSScriptRoot + '\Mods\Dynamic.psm1')
$path = $PSScriptRoot + '\Mods\Expression.psm1'
"#;
    let result = extract("scripts/run.ps1", source);

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert_eq!(
        imports(&result),
        vec![
            row(
                "Positional",
                Some("$PSScriptRoot\\Mods\\Positional.psm1"),
                "Import-Module"
            ),
            row(
                "ForwardSlash",
                Some("$PSScriptRoot/Mods/ForwardSlash.psm1"),
                "Import-Module"
            ),
            row(
                "Named",
                Some("$PSScriptRoot\\Mods\\Named.psm1"),
                "Import-Module"
            ),
            row(
                "Piped",
                Some("$PSScriptRoot\\Mods/Piped.psm1"),
                "Import-Module"
            ),
        ]
    );
    assert_eq!(
        result
            .identifiers
            .iter()
            .filter(|identifier| {
                identifier.name == "PSScriptRoot" && identifier.kind == IdentifierKind::VariableRef
            })
            .count(),
        4
    );
}

#[test]
fn computed_dot_source_does_not_invent_a_static_import_target() {
    let result = extract(
        "scripts/run.ps1",
        ". (Join-Path $PSScriptRoot 'Common.ps1')\n",
    );

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert!(imports(&result).is_empty());
}

#[test]
fn adjacent_variable_path_segments_keep_each_variable_reference() {
    let source = r#"Push-Location $PSScriptRoot\..\src\$folder
$null = New-PADrive -Path $PSScriptRoot\..\src\$folder\obj\project.assets.json -Name $folder
"#;
    let result = extract("tools/findMissingNotices.ps1", source);

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert_eq!(
        result
            .identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::VariableRef)
            .map(|identifier| identifier.name.as_str())
            .filter(|name| matches!(*name, "PSScriptRoot" | "folder"))
            .collect::<Vec<_>>(),
        vec!["PSScriptRoot", "folder", "PSScriptRoot", "folder", "folder"]
    );
}

#[test]
fn comma_lists_emit_every_import_and_keep_following_commands() {
    let result = extract(
        "scripts/run.ps1",
        "Import-Module -Name A, B\nImport-Module -Name C\nWrite-Host done\n",
    );

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert_eq!(
        imports(&result),
        vec![
            row("A", Some("A"), "Import-Module"),
            row("B", Some("B"), "Import-Module"),
            row("C", Some("C"), "Import-Module"),
        ]
    );
    assert!(result.identifiers.iter().any(|identifier| {
        identifier.name == "Write-Host" && identifier.kind == IdentifierKind::Call
    }));
}

#[test]
fn comma_lists_without_spaces_emit_every_import() {
    let result = extract(
        "scripts/run.ps1",
        "Import-Module -Name A,B\nImport-Module -Name C\nWrite-Host done\n",
    );

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert_eq!(
        imports(&result),
        vec![
            row("A", Some("A"), "Import-Module"),
            row("B", Some("B"), "Import-Module"),
            row("C", Some("C"), "Import-Module"),
        ]
    );
    assert!(result.identifiers.iter().any(|identifier| {
        identifier.name == "Write-Host" && identifier.kind == IdentifierKind::Call
    }));
}

#[test]
fn parameter_arrays_emit_every_import() {
    let result = extract(
        "scripts/run.ps1",
        "Import-Module -Name @('ArrayA', 'ArrayB')\n",
    );

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert_eq!(
        imports(&result),
        vec![
            row("ArrayA", Some("ArrayA"), "Import-Module"),
            row("ArrayB", Some("ArrayB"), "Import-Module"),
        ]
    );
    let imports = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .collect::<Vec<_>>();
    assert_eq!(imports.len(), 2);
    assert!(imports.iter().all(|symbol| symbol.body_span.is_none()));
    assert!(imports.iter().all(|symbol| symbol.body_hash.is_none()));
}

#[test]
fn comma_parameter_lists_stay_bound_to_task_and_export_rows() {
    let result = extract(
        "scripts/run.ps1",
        "task default -depends Test, Package { Write-Output task }\nExport-ModuleMember -Function Get-First, Update-Config\nWrite-Output finish\n",
    );

    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    let task = result
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "default")
        .expect("build task");
    assert_eq!(
        task.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("depends"))
            .and_then(|depends| depends.as_array())
            .map(|depends| {
                depends
                    .iter()
                    .filter_map(|dependency| dependency.as_str())
                    .collect::<Vec<_>>()
            }),
        Some(vec!["Test", "Package"])
    );
    assert_eq!(
        result
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Export)
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Get-First", "Update-Config"]
    );
    assert!(result.identifiers.iter().any(|identifier| {
        identifier.name == "Write-Output" && identifier.kind == IdentifierKind::Call
    }));
}

#[test]
fn malformed_compound_arguments_keep_parse_diagnostics() {
    let mut parser = crate::pipeline::configured_parser_for_language("powershell").unwrap();
    let tree = parser
        .parse("Import-Module $PSScriptRoot\\Mods\\X.psm1 (\n", None)
        .unwrap();

    assert!(tree.root_node().has_error());
}

#[test]
fn expression_mode_keeps_variable_and_string_as_expression_nodes() {
    let source = "$path = $PSScriptRoot + '\\Mods\\Expression.psm1'\n";
    let mut parser = crate::pipeline::configured_parser_for_language("powershell").unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "unexpected syntax nodes: {}",
        root.to_sexp()
    );
    assert!(root.to_sexp().contains("additive_expression"));
    let result = extract("scripts/run.ps1", source);
    assert!(imports(&result).is_empty());
}
