function Outer {
    function Find-Thing { "outer" }
    function Inner {
        function Find-Thing { "inner" }
        Find-Thing
    }
    Find-Thing
}

function Sibling {
    Find-Thing
}

function Get-Visible { "visible" }

function CaseCaller {
    gEt-ViSiBlE
}

function Invoke-Remote { "remote" }

function AliasOuter {
    function Lookup { "outer" }
    function AliasInner {
        Set-Alias -Name Lookup -Value Invoke-Remote
        Lookup
    }
}
