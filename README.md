# Transcription Toolkit

Lokale Transkriptionswerkzeuge mit funktional abgestimmten Apps für macOS und Windows.

## Project Layout

- `Transcription macOS/`: macOS-oriented Apple workflows with `.command` launchers and shell wrappers.
- `Transcription macOS App/`: native, offline-fähige SwiftUI-App mit `whisper.cpp`, Sprechererkennung, Editor, Audio- und Videoeingabe sowie mehreren Ausgabeformaten.
- `Transcription Windows/`: Windows-10+-App mit Medienimport, Sprechererkennung, Editor, Ergebnisbibliothek und PyInstaller-/Inno-Setup-Build.
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

Ein GitHub-Tag wie `v2.1` baut, prüft und veröffentlicht beide Plattformen über GitHub Actions:

- `Transcription-macOS-v2.1.zip.part-*` und zugehörige SHA-256-Datei
- `Transcription-Windows-v2.1.exe` und SHA-256-Datei
- `Transcription-Windows-Setup-v2.1.exe` und SHA-256-Datei

Die macOS-Archivteile werden vor dem Entpacken zusammengefügt:

```bash
cat Transcription-macOS-v2.1.zip.part-* > Transcription-macOS-v2.1.zip
```

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
- `ffmpeg` im Entwicklungsbetrieb auf `PATH`; die Release-Apps bringen es mit
- Der Entwicklungsbetrieb benötigt die in `Transcription macOS App/requirements-macos.txt` aufgeführten Python-Pakete.
- Der Offline-Release der macOS-App bündelt Laufzeit, Binärdateien und Modelle für Apple Silicon.
- Die Windows-Release-App bündelt Laufzeit und Werkzeuge; Whisper- und SpeechBrain-Gewichte werden beim ersten Einsatz in den lokalen Modellcache geladen.

Details stehen in den plattformspezifischen README-Dateien.
