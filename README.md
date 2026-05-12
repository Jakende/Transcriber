# Transcription Toolkit

Local transcription workflows using OpenAI Whisper, organized by platform.

## Project Layout

- `Transcription macOS/`: macOS-oriented Apple workflows with `.command` launchers and shell wrappers.
- `Transcription macOS App/`: native SwiftUI macOS app wrapping the same Whisper workflow.
- `Transcription Windows/`: Windows 10+ Tkinter desktop app, batch installers, and PyInstaller build helper.
- `yt_transcript_md.py`: platform-neutral YouTube subtitle-to-Markdown utility.
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
---
```

## Requirements

- Python 3.10+
- `ffmpeg` available on `PATH`
- Python packages: `torch`, `openai-whisper`

See each platform folder for detailed setup and usage notes.
