# AgentCoreutils: PowerShell cmdlets over the `acu` binary. They return objects, so results pipe into
# Where-Object, Select-Object, Export-Csv and so on. Works in Windows PowerShell 5.1 and PowerShell 7+.
#
# The binary is found in this order: $env:ACU_BINARY, a bin\acu.exe next to this module, then `acu` on PATH.

Set-StrictMode -Version 2.0

function Get-AcuBinary {
    if ($env:ACU_BINARY -and (Test-Path -LiteralPath $env:ACU_BINARY)) { return $env:ACU_BINARY }
    foreach ($name in 'acu.exe', 'acu') {
        $local = Join-Path $PSScriptRoot (Join-Path 'bin' $name)
        if (Test-Path -LiteralPath $local) { return $local }
    }
    $cmd = Get-Command acu -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    throw 'acu was not found. Install agent-coreutils (npm i -g agent-coreutils) or set $env:ACU_BINARY.'
}

# Quote one argument the way CommandLineToArgvW expects, so multi-line text and quotes arrive intact.
function ConvertTo-AcuArgument([string]$Value) {
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    $slashes = 0
    foreach ($ch in $Value.ToCharArray()) {
        if ($ch -eq '\') { $slashes++; continue }
        if ($ch -eq '"') { [void]$sb.Append('\', $slashes * 2 + 1); [void]$sb.Append('"') }
        else { [void]$sb.Append('\', $slashes); [void]$sb.Append($ch) }
        $slashes = 0
    }
    [void]$sb.Append('\', $slashes * 2)
    [void]$sb.Append('"')
    return $sb.ToString()
}

# Runs acu with UTF-8 in and out. Returns @{ ExitCode; Output; Error }.
function Invoke-Acu {
    param([string[]]$Arguments, [string]$InputText = $null)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = Get-AcuBinary
    $psi.Arguments = ($Arguments | ForEach-Object { ConvertTo-AcuArgument $_ }) -join ' '
    $psi.UseShellExecute = $false
    $psi.WorkingDirectory = (Get-Location).ProviderPath  # PowerShell's location is not the process directory
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $psi.StandardOutputEncoding = $utf8
    $psi.StandardErrorEncoding = $utf8
    $p = [System.Diagnostics.Process]::Start($psi)
    # read both pipes asynchronously so a full buffer cannot deadlock us
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    if ($null -ne $InputText) {
        $bytes = $utf8.GetBytes($InputText)
        $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    }
    $p.StandardInput.Close()
    $p.WaitForExit()
    [pscustomobject]@{ ExitCode = $p.ExitCode; Output = $outTask.Result; Error = $errTask.Result }
}

function Assert-AcuOk($Result, [string]$What) {
    if ($Result.ExitCode -ge 2) { throw ($Result.Error.Trim()) }
    if ($Result.ExitCode -eq 1 -and $Result.Error.Trim()) { throw ($Result.Error.Trim()) }
}

function Get-AcuFile {
    <#
    .SYNOPSIS  Reads files with line numbers. Returns one object per line.
    .EXAMPLE   Get-AcuFile src\a.ts -Lines 10:40
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory, Position = 0, ValueFromPipeline)][string[]]$Path, [string]$Lines, [int]$Max = 2000)
    process {
        $a = @('read') + $Path + @('--json', '--max', $Max)
        if ($Lines) { $a += @('--lines', $Lines) }
        $r = Invoke-Acu $a
        Assert-AcuOk $r 'read'
        if ($r.Error.Trim()) { Write-Warning $r.Error.Trim() }
        foreach ($f in ($r.Output | ConvertFrom-Json).files) {
            $n = [int]$f.from
            foreach ($line in ($f.content -split "`n")) {
                if ($n -gt $f.to) { break }
                [pscustomobject]@{ Path = $f.path; Line = $n; Text = $line }
                $n++
            }
        }
    }
}

function Search-AcuCode {
    <#
    .SYNOPSIS  Searches file contents (regular expression by default). Returns Path, Line, Text objects.
    .EXAMPLE   Search-AcuCode 'TODO|FIXME' -Path src -Type ts -Max 50
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory, Position = 0)][string]$Pattern,
        [Parameter(Position = 1)][string[]]$Path = @('.'),
        [switch]$Fixed, [switch]$IgnoreCase, [switch]$Word, [string]$Type, [string[]]$Glob, [int]$Max = 200, [switch]$NoIgnore
    )
    $a = @('grep', '--json', '--max', $Max, '-e', $Pattern)
    if ($Fixed) { $a += '-F' }
    if ($IgnoreCase) { $a += '-i' }
    if ($Word) { $a += '-w' }
    if ($Type) { $a += @('--type', $Type) }
    foreach ($g in $Glob) { $a += @('--glob', $g) }
    if ($NoIgnore) { $a += '--no-ignore' }
    $r = Invoke-Acu ($a + $Path)
    Assert-AcuOk $r 'grep'
    if ($r.ExitCode -eq 1) { return }
    ($r.Output | ConvertFrom-Json).matches | ForEach-Object { [pscustomobject]@{ Path = $_.path; Line = [int]$_.line; Text = $_.text } }
}

function Find-AcuFile {
    <#
    .SYNOPSIS  Finds files by glob pattern. Returns paths.
    .EXAMPLE   Find-AcuFile '**/*.test.ts' -In src
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory, Position = 0)][string[]]$Pattern, [string]$In = '.', [int]$Max = 500)
    $r = Invoke-Acu (@('glob', '--json', '--max', $Max, '--in', $In) + $Pattern)
    Assert-AcuOk $r 'glob'
    if ($r.ExitCode -eq 1) { return }
    ($r.Output | ConvertFrom-Json).paths
}

function Get-AcuOutline {
    <#
    .SYNOPSIS  Symbols of source files: Path, Line, Depth, Kind, Name.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory, Position = 0, ValueFromPipeline)][string[]]$Path)
    process {
        $r = Invoke-Acu (@('outline', '--json') + $Path)
        Assert-AcuOk $r 'outline'
        foreach ($f in ($r.Output | ConvertFrom-Json).files) {
            foreach ($s in $f.symbols) {
                [pscustomobject]@{ Path = $f.path; Line = [int]$s.line; Depth = [int]$s.depth; Kind = $s.kind; Name = $s.name }
            }
        }
    }
}

function Get-AcuSkim {
    <#
    .SYNOPSIS  The shape of a codebase as text, sized to a token budget.
    #>
    [CmdletBinding()]
    param([Parameter(Position = 0)][string]$Path = '.', [int]$Depth = 0, [int]$Budget = 4000)
    $r = Invoke-Acu @('skim', $Path, '--depth', $Depth, '--budget', $Budget)
    Assert-AcuOk $r 'skim'
    $r.Output.TrimEnd()
}

function Find-AcuDefinition {
    <#
    .SYNOPSIS  Where a symbol is defined: Path, Line, Kind, Text.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory, Position = 0)][string]$Symbol, [Parameter(Position = 1)][string[]]$Path = @('.'), [switch]$IgnoreCase)
    $a = @('find-def', '--json', $Symbol)
    if ($IgnoreCase) { $a += '-i' }
    $r = Invoke-Acu ($a + $Path)
    Assert-AcuOk $r 'find-def'
    if ($r.ExitCode -eq 1) { return }
    ($r.Output | ConvertFrom-Json).definitions | ForEach-Object { [pscustomobject]@{ Path = $_.path; Line = [int]$_.line; Kind = $_.kind; Text = $_.text } }
}

function Edit-AcuFile {
    <#
    .SYNOPSIS  Exact-string replace. Writes nothing and throws if -Old is missing or ambiguous (unless -All).
    .EXAMPLE   Edit-AcuFile src\a.ts -Old 'retries = 3' -New 'retries = 5'
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param([Parameter(Mandatory, Position = 0)][string]$Path, [Parameter(Mandatory)][string]$Old, [Parameter(Mandatory)][AllowEmptyString()][string]$New, [switch]$All)
    $oldFile = [System.IO.Path]::GetTempFileName()
    $newFile = [System.IO.Path]::GetTempFileName()
    try {
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($oldFile, $Old, $utf8)
        [System.IO.File]::WriteAllText($newFile, $New, $utf8)
        $a = @('edit', $Path, '--old-file', $oldFile, '--new-file', $newFile)
        if ($All) { $a += '--all' }
        if (-not $PSCmdlet.ShouldProcess($Path, 'Edit')) { $a += '--dry-run' }
        $r = Invoke-Acu $a
        if ($r.ExitCode -ne 0) { throw ($r.Error.Trim()) }
        $r.Output.Trim()
    }
    finally { Remove-Item -LiteralPath $oldFile, $newFile -ErrorAction SilentlyContinue }
}

function Set-AcuContent {
    <#
    .SYNOPSIS  Writes a file atomically; keeps an existing file's line endings and BOM.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    param([Parameter(Mandatory, Position = 0)][string]$Path, [Parameter(Mandatory, Position = 1, ValueFromPipeline)][AllowEmptyString()][string]$Value, [switch]$Append, [switch]$Raw)
    begin { $parts = New-Object System.Collections.Generic.List[string] }
    process { $parts.Add($Value) }
    end {
        $text = $parts -join ''
        if (-not $PSCmdlet.ShouldProcess($Path, 'Write')) { return }
        $a = @('write', $Path)
        if ($Append) { $a += '--append' }
        if ($Raw) { $a += '--raw' }
        $r = Invoke-Acu $a -InputText $text
        if ($r.ExitCode -ne 0) { throw ($r.Error.Trim()) }
        $r.Output.Trim()
    }
}

Export-ModuleMember -Function Get-AcuFile, Search-AcuCode, Find-AcuFile, Get-AcuOutline, Get-AcuSkim, Find-AcuDefinition, Edit-AcuFile, Set-AcuContent, Invoke-Acu
