@echo off
setlocal
cd /d "%~dp0"

if exist ".venv\Scripts\python.exe" (
    ".venv\Scripts\python.exe" transcription_windows_app.py
) else (
    py -3 transcription_windows_app.py
)

pause
