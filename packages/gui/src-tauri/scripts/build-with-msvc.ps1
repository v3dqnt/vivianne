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
    Write-Error "vcvars64.bat not found at $vcvars — install VS 2019 Build Tools with the C++ workload."
    exit 1
}

$argLine = $CargoArgs -join " "
# cmd /c: import the vcvars environment, then run cargo in the same process.
cmd /c "`"$vcvars`" >nul 2>&1 && cargo $argLine"
exit $LASTEXITCODE
