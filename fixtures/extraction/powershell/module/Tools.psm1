#Requires -Modules Az.Accounts, @{ ModuleName = 'Pester'; ModuleVersion = '5.0' }
using namespace System.Text

Import-Module "$PSScriptRoot\Helpers.psm1" -Force
Import-Module -Name Az.Storage -MinimumVersion 2.0
. (Join-Path $PSScriptRoot 'Common.ps1')

function Get-Public { Get-Internal }
function Get-Internal { 'internal' }

New-Alias gp Get-Public
Export-ModuleMember -Function 'Get-Public' -Alias 'gp'

function Use-Alias { gp }
