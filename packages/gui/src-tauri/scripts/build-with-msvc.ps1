# Runs a cargo command inside the VS 2019 Build Tools developer environment so
# CMake's Ninja generator (see ../.cargo/config.toml) can find cl.exe/link.exe.
# Needed because whisper-rs-sys compiles whisper.cpp via CMake, and CMake's
# Visual Studio generators fail to enumerate the VS instances on this machine.
#
# Usage:
#   .\scripts\build-with-msvc.ps1 check
#   .\scripts\build-with-msvc.ps1 build --release
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs = @("check")
)

$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    Write-Error "vcvars64.bat not found at $vcvars - install VS 2019 Build Tools with the C++ workload."
    exit 1
}

# Import the vcvars environment variables into the current PowerShell process
$cmd = '"{0}" >nul 2>&1 && set' -f $vcvars
$vars = cmd /c $cmd
foreach ($line in $vars) {
    if ($line -match "^([^=]+)=(.*)$") {
        $name = $Matches[1]
        $val = $Matches[2]
        [Environment]::SetEnvironmentVariable($name, $val, "Process")
    }
}

# Split INCLUDE and construct BINDGEN_EXTRA_CLANG_ARGS so bindgen/clang finds stdio.h
$clangIncludes = @('-I"C:\Program Files\LLVM\lib\clang\22\include"')
if ($env:INCLUDE) {
    foreach ($path in $env:INCLUDE -split ';') {
        if ($path) {
            $clangIncludes += '-I"{0}"' -f $path
        }
    }
}
$env:BINDGEN_EXTRA_CLANG_ARGS = $clangIncludes -join ' '

# Run cargo in the prepared environment
cargo $CargoArgs
exit $LASTEXITCODE
