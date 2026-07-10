Describe "powershell roles" {
    BeforeEach {
        $state = 1
    }

    Context "addition" {
        It "extracts a Pester test case" {
            1 + 1 | Should -Be 2
        }
    }
}

function Get-Total {
    return 2
}

Context.Helper "ordinary dotted command" {
    Get-Total
}
