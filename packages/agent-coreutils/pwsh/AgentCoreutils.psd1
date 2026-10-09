@{
    RootModule        = 'AgentCoreutils.psm1'
    ModuleVersion     = '0.1.0'
    GUID              = '6f1c3e2a-5b0d-4c7e-9a44-1d2f7a9b8c01'
    Author            = 'agent-coreutils contributors'
    Description       = 'PowerShell cmdlets over the acu binary: fast, model-friendly file tools that return objects.'
    PowerShellVersion = '5.1'
    FunctionsToExport = @('Get-AcuFile', 'Search-AcuCode', 'Find-AcuFile', 'Get-AcuOutline', 'Get-AcuSkim', 'Find-AcuDefinition', 'Edit-AcuFile', 'Set-AcuContent', 'Invoke-Acu')
    CmdletsToExport   = @()
    VariablesToExport = @()
    AliasesToExport   = @()
}
