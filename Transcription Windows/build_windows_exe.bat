@echo off
setlocal
cd /d "%~dp0"

if exist ".venv\Scripts\activate.bat" (
    call .venv\Scripts\activate.bat
)

python scripts\generate_windows_icon.py assets\AppIcon.ico
if errorlevel 1 goto error

python -m PyInstaller --noconfirm --onefile --windowed --icon assets\AppIcon.ico --name "Transcription Windows" transcription_windows_app.py
if errorlevel 1 goto error

echo.
echo Build complete:
echo dist\Transcription Windows.exe
pause
exit /b 0

:error
echo.
echo Build failed.
pause
exit /b 1
