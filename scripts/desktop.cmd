@echo off
setlocal
set "MODE=%~1"
if /I not "%MODE%"=="dev" if /I not "%MODE%"=="build" exit /b 1
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "VSINSTALL="
if exist "%VSWHERE%" for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSINSTALL=%%i"
if "%VSINSTALL%"=="" exit /b 2
call "%VSINSTALL%\Common7\Tools\VsDevCmd.bat" -arch=x64
if errorlevel 1 exit /b %errorlevel%
if /I "%MODE%"=="build" goto build
call npm run tauri -- dev
if errorlevel 1 exit /b %errorlevel%
exit /b %errorlevel%
:build
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-release.ps1"
exit /b %errorlevel%
