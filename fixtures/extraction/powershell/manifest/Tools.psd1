@{
    RootModule        = 'Tools.psm1'
    ModuleVersion     = '1.2.0'
    GUID              = '5f2a4c1e-0000-4000-8000-000000000001'
    RequiredModules   = @('Az.Accounts', @{ ModuleName = 'Pester'; ModuleVersion = '5.0.0' })
    FunctionsToExport = @('Get-Public')
    AliasesToExport   = @('gp')
    PrivateData = @{ PSData = @{ Tags = @('tools') } }
}
