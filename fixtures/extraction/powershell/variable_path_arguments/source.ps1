Import-Module $PSScriptRoot\Mods\X.psm1
Import-Module $PSScriptRoot/Mods/Y.psm1
Import-Module -Name $PSScriptRoot\Mods\Named.psm1
Import-Module -Name A, B
Import-Module -Name C
Import-Module -Name D,E
Import-Module -Name @('ArrayA', 'ArrayB')
Write-Host after-imports
$cgManifestPath = (Resolve-Path -Path $PSScriptRoot\..\tools\cgmanifest.json).ProviderPath
Import-Module ($PSScriptRoot + '\Mods\Dynamic.psm1')
