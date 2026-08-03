# Transcription Toolkit

Local transcription workflows using OpenAI Whisper, organized by platform.

## Project Layout

- `Transcription macOS/`: macOS-oriented Apple workflows with `.command` launchers and shell wrappers.
- `Transcription macOS App/`: native, offline-fähige SwiftUI-App mit `whisper.cpp`, Sprechererkennung, Editor, Audio- und Videoeingabe sowie mehreren Ausgabeformaten.
- `Transcription Windows/`: Windows 10+ Tkinter desktop app, batch installers, and PyInstaller build helper.
- `AGENTS.md`: contributor and agent guidelines.

## Quick Start

macOS:

```bash
cd "Transcription macOS"
./run_transcript_de.sh
./run_transcript_en.sh
```

macOS app:

```bash
cd "Transcription macOS App"
./script/build_and_run.sh
```

Windows:

```bat
cd "Transcription Windows"
install_windows.bat
run_windows_app.bat
```

## Releases

GitHub tag `v1.0` builds and publishes a release through GitHub Actions:

- `Transcription-macOS-v1.0.zip`
- `Transcription Windows.exe`

## Output Format

Generated Markdown is optimized for Obsidian and includes YAML front matter:

```yaml
---
created: "2026-05-12 17:30"
model: "large"
device: "mps"
source_file: "audio.mp3"
fps_timecode: 25
timecodes: true
language: "de"
engine: "whisper.cpp"
diarization: true
speaker_count: 2
---
```

## Requirements

- Python 3.10+
- `ffmpeg` available on `PATH`
- Der Entwicklungsbetrieb benötigt die in `Transcription macOS App/requirements-macos.txt` aufgeführten Python-Pakete.
- Der Offline-Release der macOS-App bündelt Laufzeit, Binärdateien und Modelle für Apple Silicon.

See each platform folder for detailed setup and usage notes.
