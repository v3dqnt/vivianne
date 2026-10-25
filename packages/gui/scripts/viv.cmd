@echo off
rem viv: Vivianne's command line. Runs the app's own binary with your arguments.
rem Put this folder on PATH, or copy this file next to the installed Vivianne.exe.
set "VIV_EXE=%~dp0..\src-tauri\target\release\app.exe"
if not exist "%VIV_EXE%" set "VIV_EXE=%~dp0..\src-tauri\target\debug\app.exe"
if not exist "%VIV_EXE%" set "VIV_EXE=%~dp0Vivianne.exe"
"%VIV_EXE%" %*
