# Repository Guidelines

## Project Structure & Module Organization

This repository contains local transcription utilities organized by platform.

- `Transcription macOS/`: macOS workflows, `.command` launchers, and shell wrappers.
- `Transcription macOS App/`: SwiftPM/SwiftUI macOS app and Python transcription helper.
- `Transcription Windows/`: Windows 10+ Tkinter app, batch helpers, and Windows docs.
- `README.md`: repository overview and platform entry points.

Generated files such as `__pycache__/`, `.DS_Store`, `dist/`, and build folders should not be treated as source.

## Build, Test, and Development Commands

Run syntax checks before handing off changes:

```bash
python3 -m py_compile "Transcription macOS/transkript_any_audio_de.py" "Transcription macOS/transkript_any_audio_en.py"
python3 -m py_compile "Transcription macOS App/Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py"
python3 -m py_compile "Transcription Windows/transcription_windows_app.py"
```

Run the macOS workflows:

```bash
cd "Transcription macOS"
./run_transcript_de.sh
./run_transcript_en.sh
```

On Windows, use:

```bat
cd "Transcription Windows"
install_windows.bat
run_windows_app.bat
build_windows_exe.bat
```

`ffmpeg` must be available on `PATH` for audio/video decoding.

Build the native macOS app:

```bash
cd "Transcription macOS App"
./script/build_and_run.sh --no-run
```

## Coding Style & Naming Conventions

Use Python 3.10+ compatible code, 4-space indentation, clear function names, and concise GUI text. Keep platform-specific behavior inside the matching platform folder. Use `Path` for new filesystem-heavy code where practical, but avoid broad refactors of existing scripts unless needed.

Markdown outputs should preserve Obsidian-compatible YAML front matter, including `created`, `model`, `device`, `source_file`, `fps_timecode`, and `timecodes`.

## Testing Guidelines

There is no formal test suite yet. At minimum, run `py_compile` on changed Python files. For transcription changes, manually test with a short audio file and verify model selection, bulk processing, Markdown creation, optional timecodes, and valid YAML front matter.

## Commit & Pull Request Guidelines

No Git history is available in this folder, so use simple imperative commit messages:

```text
Add Windows transcription app
Organize platform-specific workflows
```

Pull requests should include a short description, affected platform folder, manual test steps, and screenshots for GUI changes. Note platform-specific limitations and dependency requirements.

## Agent-Specific Instructions

Keep `Transcription macOS/`, `Transcription macOS App/`, and `Transcription Windows/` workflows independent unless the user asks to synchronize behavior.
