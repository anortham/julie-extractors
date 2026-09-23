<#
.SYNOPSIS
    Deploys the inventory service.
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

class Invoice {
    [DscProperty(Key)]
    [string]$Id
    [ValidateNotNullOrEmpty()][decimal]$Total
    hidden [bool]$hiddenFlag
}

class Billing {
    [Invoice] Create() { return [Invoice]::new() }
    static [Invoice[]] All() { return @() }
    Hidden [void] Reset() { $this.Create() }
}

function global:Get-Invoice {
    [CmdletBinding()]
    [OutputType([Invoice])]
    param(
        # The invoice identifier.
        [Parameter(Mandatory = $true, ValueFromPipeline = $true)]
        [string] $Id
    )
    function Format-Line {
        param([int]$Width)
        $Width
    }
    $inv = [Invoice]$null
    $sb = { param($Item) $Item }
    format-line -Width 10
    $inv
}

function Send-Invoice {
    & Get-Invoice -Id 'A1'
    & "$PSScriptRoot\publish.ps1" -Task Test
    Invoke-RestMethod -Method Post -Uri 'https://api.contoso.com/invoices' -ContentType 'application/json'
    irm https://api.contoso.com/health
    Invoke-Sqlcmd -ServerInstance 'db01' -Query 'SELECT Id FROM dbo.Invoices'
    $names = Get-Process | Select-Object -ExpandProperty Name
    Write-Host "a|b"
    dotnet restore .
    Invoke-Compile -Configuration Release
}

Configuration InvoiceServer {
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

task default -depends Test,Package
task Package -depends Clean {
    Send-Invoice
}
