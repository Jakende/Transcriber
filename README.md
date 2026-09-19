# Transcriber

[![Release](https://img.shields.io/github/v/release/Jakende/Transcriber?display_name=tag&sort=semver)](https://github.com/Jakende/Transcriber/releases/latest)
[![Release builds](https://github.com/Jakende/Transcriber/actions/workflows/release.yml/badge.svg)](https://github.com/Jakende/Transcriber/actions/workflows/release.yml)

Local audio and video transcription for macOS and Windows. Transcriber combines Whisper transcription, optional speaker diarization, a transcript editor, podcast and YouTube import, and Obsidian-ready exports in native desktop apps.

## Download

Get the current version from the [GitHub Releases page](https://github.com/Jakende/Transcriber/releases/latest).

| Platform | Recommended download | Notes |
| --- | --- | --- |
| Windows 10 or later | `Transcription-Windows-Setup-v*.exe` | Installer with Start menu entry; the easiest option for most people. |
| Windows 10 or later | `Transcription-Windows-v*.exe` | Portable app; no installation required. |
| macOS 13 or later on Apple Silicon | `Transcription-macOS-v*.zip.part-*` | Join the archive parts before extracting; see below. |

Windows may show a Microsoft SmartScreen warning because the app is not code-signed. Select **More info** and then **Run anyway** only after downloading the file from this repository's Releases page.

### Install on Windows

1. Download `Transcription-Windows-Setup-v*.exe`.
2. Run the installer and follow the prompts.
3. Open **Transcription Windows** from the Start menu.

The portable `.exe` is an alternative when you do not want to install the app: place it in any folder and run it directly.

### Install on macOS

Download every `Transcription-macOS-v*.zip.part-*` file and the matching `.sha256` file. In Terminal, from the download folder:

```bash
cat Transcription-macOS-v*.zip.part-* > Transcription-macOS.zip
shasum -a 256 -c Transcription-macOS-v*.zip.sha256
ditto -x -k Transcription-macOS.zip .
```

Move **Transcription macOS.app** to `/Applications` and open it. The app is ad-hoc signed, not notarized; macOS may ask you to confirm the first launch in System Settings.

## Features

- Batch queue with file picker and drag-and-drop
- Local audio and video transcription with Whisper Small, Medium, and Turbo
- German, English, and automatic mixed-language recognition
- Optional speaker diarization with automatic or chosen speaker counts
- Speaker names, samples, merging, and post-processing tools
- Transcript editor with search, split/merge/delete, undo/redo, backups, playback loops, and adjustable speed
- Persistent result library with search, archive, trash, and recovered editing state
- RSS, PodcastIndex, and YouTube media import
- VTT import with optional linked media
- Shared glossary context and bilingual terminology review
- Markdown, VTT, SRT, TXT, and CSV export without silent overwrites
- Obsidian-compatible YAML front matter, including source and podcast metadata

## Local processing and first use

The macOS release bundle includes its transcription runtime, models, and media tools, so transcription, diarization, and terminology review work offline after installation.

The Windows release includes the application runtime, FFmpeg/FFprobe/FFplay, Deno, and yt-dlp. Whisper and SpeechBrain model weights are downloaded to `%LOCALAPPDATA%\\Transcription Windows\\models` the first time they are used, then reused locally. The first model download therefore requires an internet connection and several gigabytes of free disk space.

For video files, Transcriber processes the first audio stream. Files without audio are skipped and reported in the activity log.

## Development

The repository keeps platform-specific workflows separate:

- [`Transcription macOS/`](Transcription%20macOS/) — shell-based macOS workflows.
- [`Transcription macOS App/`](Transcription%20macOS%20App/) — native SwiftUI app for Apple Silicon.
- [`Transcription Windows/`](Transcription%20Windows/) — native Windows desktop app, build scripts, and installer.

### macOS app

```bash
cd "Transcription macOS App"
./script/build_and_run.sh --no-run
```

For a complete distributable bundle, use `./script/build_and_run.sh --bundle --no-run`.

### Windows app

```bat
cd "Transcription Windows"
install_windows.bat
run_windows_app.bat
```

Build the portable executable and installer with:

```bat
build_windows_exe.bat
build_windows_installer.bat
```

### Checks

From the repository root:

```bash
python3 -m py_compile "Transcription Windows/transcription_windows_app.py" "Transcription Windows/windows_services.py"
python3 -m py_compile "Transcription macOS App/Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py"
python3 -m unittest tests/test_macos_transcription_backend.py tests/test_windows_services.py tests/test_windows_safe_streams.py
```

## Releases

Pushing a version tag such as `v2.2`, or dispatching the **Release** workflow with a tag, validates shared Python sources and builds both platform packages on GitHub Actions. Each release includes SHA-256 checksums for its published assets.

See [NOTICE](NOTICE) and [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for attribution and license information.
