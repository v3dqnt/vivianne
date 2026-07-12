@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 (
  echo VSDEVCMD_FAILED
  exit /b 1
)
where cl.exe
cargo check --message-format=short
