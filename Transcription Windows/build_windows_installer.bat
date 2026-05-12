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

ISCC.exe installer\TranscriptionWindows.iss
if errorlevel 1 goto error

echo.
echo Installer complete:
echo dist\Transcription-Windows-Setup-v1.0.exe
if /i not "%CI%"=="true" pause
exit /b 0

:error
echo.
echo Installer build failed.
if /i not "%CI%"=="true" pause
exit /b 1
