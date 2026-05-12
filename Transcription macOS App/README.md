# Transcription macOS App

Native macOS SwiftUI app for bulk audio and video transcription with OpenAI Whisper.

## Features

- Select multiple media files
- Choose German or English transcription
- Choose Whisper/Wispr model (`large` by default)
- Enable or disable timecodes before starting
- Set a buffer between files
- Save Markdown next to source files or in a selected output folder
- Write Obsidian-compatible YAML front matter

## Requirements

- macOS 13+
- Xcode command line tools / SwiftPM
- Python 3.10+
- `ffmpeg` on `PATH`
- Python packages: `torch`, `openai-whisper`

## Build and Run

```bash
cd "Transcription macOS App"
./script/build_and_run.sh
```

Build without launching:

```bash
./script/build_and_run.sh --no-run
```

The app bundle is created at:

```text
dist/Transcription macOS.app
```
