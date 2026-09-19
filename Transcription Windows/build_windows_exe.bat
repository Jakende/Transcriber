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

python scripts\prepare_ffmpeg.py
if errorlevel 1 goto error

python scripts\prepare_deno.py
if errorlevel 1 goto error

python -m PyInstaller ^
    --noconfirm ^
    --onefile ^
    --windowed ^
    --icon assets\AppIcon.ico ^
    --name "Transcription Windows" ^
    --paths "..\Transcription macOS App\Sources\TranscriptionMacOSApp\Resources" ^
    --add-binary "vendor\ffmpeg\ffmpeg.exe;." ^
    --add-binary "vendor\ffmpeg\ffprobe.exe;." ^
    --add-binary "vendor\ffmpeg\ffplay.exe;." ^
    --add-binary "vendor\deno\deno.exe;." ^
    --hidden-import torch ^
    --hidden-import whisper ^
    --hidden-import tiktoken ^
    --hidden-import transcription_backend ^
    --collect-submodules transcription_backend ^
    --collect-all torch ^
    --collect-all torchaudio ^
    --collect-all whisper ^
    --collect-all tiktoken ^
    --collect-all speechbrain ^
    --collect-all silero_vad ^
    --collect-all sklearn ^
    --collect-all spacy ^
    --collect-all de_core_news_sm ^
    --collect-all en_core_web_sm ^
    --collect-all yt_dlp ^
    --collect-all keyring ^
    --collect-all tkinterdnd2 ^
    --copy-metadata openai-whisper ^
    --copy-metadata speechbrain ^
    --copy-metadata spacy ^
    --copy-metadata yt-dlp ^
    transcription_windows_app.py
if errorlevel 1 goto error

"dist\Transcription Windows.exe" --smoke-test
if errorlevel 1 goto error

echo.
echo Build complete:
echo dist\Transcription Windows.exe
echo.
echo The executable bundles torch and openai-whisper from the build environment.
echo The executable also bundles ffmpeg, ffprobe, ffplay, Deno, speaker analysis,
echo spaCy language models, yt-dlp, the editor, and media-import support.
if /i not "%CI%"=="true" pause
exit /b 0

:error
echo.
echo Build failed.
if /i not "%CI%"=="true" pause
exit /b 1
