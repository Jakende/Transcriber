@echo off
setlocal
cd /d "%~dp0"

if not exist "dist\Transcription Windows.exe" (
    call build_windows_exe.bat
    if errorlevel 1 goto error
)

where ISCC.exe >nul 2>&1
if errorlevel 1 (
    echo.
    echo Inno Setup compiler ISCC.exe was not found on PATH.
    echo Install Inno Setup from https://jrsoftware.org/isinfo.php
    echo or let GitHub Actions build the installer.
    if /i not "%CI%"=="true" pause
    exit /b 1
)

set "APP_VERSION=2.1"
set "OUTPUT_NAME=Transcription-Windows-Setup-v2.1"
if not "%RELEASE_TAG%"=="" (
    set "APP_VERSION=%RELEASE_TAG:v=%"
    set "OUTPUT_NAME=Transcription-Windows-Setup-%RELEASE_TAG%"
)

ISCC.exe /DMyAppVersion=%APP_VERSION% /DMyOutputBaseFilename=%OUTPUT_NAME% installer\TranscriptionWindows.iss
if errorlevel 1 goto error

echo.
echo Installer complete:
echo dist\%OUTPUT_NAME%.exe
if /i not "%CI%"=="true" pause
exit /b 0

:error
echo.
echo Installer build failed.
if /i not "%CI%"=="true" pause
exit /b 1
