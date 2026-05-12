@echo off
setlocal
cd /d "%~dp0"

if not exist ".venv\Scripts\python.exe" (
    py -3 -m venv .venv
    if errorlevel 1 goto error
)

call .venv\Scripts\activate.bat
python -m pip install --upgrade pip
if errorlevel 1 goto error

pip install -r requirements.txt
if errorlevel 1 goto error

python scripts\generate_windows_icon.py assets\AppIcon.ico
if errorlevel 1 goto error

python -m PyInstaller ^
    --noconfirm ^
    --onefile ^
    --windowed ^
    --icon assets\AppIcon.ico ^
    --name "Transcription Windows" ^
    --hidden-import torch ^
    --hidden-import whisper ^
    --hidden-import tiktoken ^
    --collect-all torch ^
    --collect-all whisper ^
    --collect-all tiktoken ^
    transcription_windows_app.py
if errorlevel 1 goto error

echo.
echo Build complete:
echo dist\Transcription Windows.exe
echo.
echo The executable bundles torch and openai-whisper from the build environment.
echo ffmpeg still needs to be available on PATH for media decoding.
pause
exit /b 0

:error
echo.
echo Build failed.
pause
exit /b 1
