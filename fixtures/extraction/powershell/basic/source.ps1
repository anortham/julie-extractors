# Copyright (c) Contoso. This header documents the file, not the import.

# Shared helper module.
Import-Module Contoso.Helpers

function Invoke-Helper {
    param([int]$Value)
    return $Value + 1
}

function Invoke-Run {
    param([int]$Value)
    return Invoke-Helper $Value
}

function Evaluate {
    [CmdletBinding()]
    param([int]$Count, [bool]$Enabled)
    $total = 0
    if ($Enabled) {
        for ($i = 1; $i -le $Count; $i++) {
            $total += $i
        }
    } elseif ($Count -gt 0) {
        $total = 1
    }
    return $total
}

function Get-Filtered {
    Get-Process | Select-Object -First 1
}

[Dictionary[string, List[int]]]$script:WorkerIndex = @{}

class Worker {
    [int]$Id

    Worker([int]$id) {
        $this.Id = $id
    }

    [int] Run() {
        return Invoke-Helper $this.Id
    }
}

function Get-Name {
    [CmdletBinding()]
    param(
        [Parameter()]
        [string]
        $Name
    )
}

class Widget {
    [string]$Title

    Widget() {}

    [void] Run([Foo]$f) {
        $this.Run($f)
        $other.Run($f)
    }
}

function Use-Facts {
    [System.Collections.Generic.List[string]]$items = @()
    $w = [Widget]::new()
    $n = New-Object Widget
    $g = Get-Thing
}

function Use-Arrays {
    [string[]]$names = @()
    [int[,]]$grid = $null
}

function Get-First {
    <#
    .SYNOPSIS
    Returns the first item.
    #>
    [CmdletBinding()]
    [OutputType([Widget])]
    param([Widget[]] $Items)
    $Items[0]
}

# A repository with a cross-file base class.
class Repo : BaseRepo, IDisposable {
    [Widget] $Current

    [void] Dispose() {
        Invoke-Helper 1
        Write-AuditLog -Message 'disposed'
    }
}

class Gadget : Widget {
    [void] Touch() { [Widget]::Count++ }
}

function Update-Config {
    param([Widget] $Config)
    # Upper bound for retries.
    $MAX = 5
    $MAX = 6
    $Config.Title = 'n'
    $Config['Retries'] = 5
    $env:BUILD_ID = '42'
    $Config.Run($null)
    $made = New-Object -TypeName Gadget
    $cast = [Widget]$made
    if ($cast -is [Gadget]) { }
    try { } catch [System.IO.IOException] { }
    Get-ChildItem $PSScriptRoot | ForEach-Object { $_.Name } | Where-Object { $_ -ne $MAX }
}

Export-ModuleMember -function Get-First, Update-Config
