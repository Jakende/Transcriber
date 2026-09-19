@echo off
setlocal
cd /d "%~dp0"

py -3 -m venv .venv
if errorlevel 1 goto error

call .venv\Scripts\activate.bat
python -m pip install --upgrade pip
pip install -r requirements.txt
if errorlevel 1 goto error

python scripts\prepare_ffmpeg.py
if errorlevel 1 goto error

python scripts\prepare_deno.py
if errorlevel 1 goto error

echo.
echo Installation complete.
echo Start the app with run_windows_app.bat.
pause
exit /b 0

:error
echo.
echo Installation failed.
echo Make sure Python 3.10+ is installed and an internet connection is available.
pause
exit /b 1
