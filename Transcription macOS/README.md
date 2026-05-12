# Transcription macOS

This folder contains macOS-oriented audio and video transcription workflows using OpenAI Whisper.

## Included Scripts

- `transkript_any_audio_de.py`: transcribes selected media in German (`language="de"`)
- `transkript_any_audio_en.py`: transcribes selected media in English (`language="en"`)

Both scripts:

- open a file picker dialog (`tkinter`) to choose one or more audio or video files
- ask which Whisper model to load (`large` by default)
- ask whether Markdown output should include timecodes
- auto-pick the best device (`mps`, `cuda`, fallback `cpu`)
- create one Markdown file next to each selected media file
- optionally include frame-based timecodes (`FPS=25`)
- process multiple selected files in sequence with a short buffer between files

## Supported Formats

Audio:

- `.wav`
- `.mp3`
- `.m4a`
- `.flac`
- `.aac`
- `.ogg`
- `.wma`

Video:

- `.mp4`
- `.mov`
- `.MOV`
- `.m4v`
- `.avi`
- `.mkv`
- `.webm`

## Requirements

- Python 3.10+
- `tkinter` available in your Python installation
- `ffmpeg` available on `PATH` for media decoding
- Python packages:
  - `torch`
  - `openai-whisper`

Install dependencies:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip
pip install torch openai-whisper
```

## Quick Start

Use the shell wrappers in this folder.

German transcription:

```bash
cd "Transcription macOS"
./run_transcript_de.sh
```

English transcription:

```bash
cd "Transcription macOS"
./run_transcript_en.sh
```

You can also double-click the `.command` files in Finder.

## Output

For an input file like:

```text
/path/to/interview.wav
```

the output will be:

```text
/path/to/interview.md
```

Markdown includes:

- Obsidian-compatible YAML front matter
- `created` as transcription date
- `model` and `device`
- `source_file`
- `fps_timecode`
- `timecodes`
- transcript text, optionally with timecoded blocks

## Troubleshooting

- `ModuleNotFoundError: whisper` or `torch`
  - activate your environment and install dependencies again.
- No file dialog appears
  - ensure you run in a desktop session with GUI access.
- Very slow transcription
  - CPU fallback is active. On macOS, MPS support may speed this up.
