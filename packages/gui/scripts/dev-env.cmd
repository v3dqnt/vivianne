@echo off
REM Locates and activates the MSVC build environment portably, instead of
REM hardcoding a specific Visual Studio version/path. Uses vswhere.exe
REM (shipped with any VS 2017+ or Build Tools install at a stable, versionless
REM location) to find whatever VS the machine actually has, then forwards all
REM arguments to the tauri CLI inside that environment.
setlocal
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
  echo [dev-env] vswhere.exe not found. Install Visual Studio 2017+ or the C++ Build Tools.
  exit /b 1
)
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSPATH=%%i"
if not defined VSPATH (
  if exist "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\Common7\Tools\VsDevCmd.bat" (
    set "VSPATH=C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools"
  ) else if exist "C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\Common7\Tools\VsDevCmd.bat" (
    set "VSPATH=C:\Program Files (x86)\Microsoft Visual Studio\2019\Community"
  ) else if exist "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat" (
    set "VSPATH=C:\Program Files\Microsoft Visual Studio\2022\Community"
  ) else if exist "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" (
    set "VSPATH=C:\Program Files\Microsoft Visual Studio\2022\BuildTools"
  )
)
if not defined VSPATH (
  echo [dev-env] No VS installation with the MSVC C++ toolchain found.
  exit /b 1
)
call "%VSPATH%\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64 >nul
tauri %*
