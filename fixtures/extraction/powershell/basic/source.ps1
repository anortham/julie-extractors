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
