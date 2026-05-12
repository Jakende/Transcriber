# Transcription Windows

Windows-optimierte Desktop-App fuer lokale Audio- und Video-Transkription mit OpenAI Whisper.

## Voraussetzungen

- Windows 10 oder neuer
- Python 3.10 oder neuer
- ffmpeg auf `PATH`
- Internet beim ersten Modell-Download

## Installation

1. `install_windows.bat` doppelklicken.
2. Danach `run_windows_app.bat` doppelklicken.

## Bedienung

1. Dateien auswaehlen.
2. Sprache auswaehlen: `de` oder `en`.
3. Whisper/Wispr Modell auswaehlen, Standard ist `large`.
4. Auswaehlen, ob Timecodes in das Markdown geschrieben werden sollen.
5. Optional Zielordner festlegen.
6. `Transkription starten` klicken.

Die App transkribiert alle Dateien nacheinander und wartet zwischen Dateien den eingestellten Buffer ab.

## Ausgabe

Pro Quelldatei entsteht eine Markdown-Datei mit:

- Obsidian-kompatiblem YAML Front Matter
- `created` als Datum der Transkription
- `model`
- `device`
- `source_file`
- `fps_timecode`
- `timecodes`
- optional timecodierten Segmenten

## App Icon

The Windows build uses `assets\AppIcon.ico`, generated from the same black-and-white speaking smiley icon style as the macOS app:

```bat
python scripts\generate_windows_icon.py assets\AppIcon.ico
```

## EXE bauen

Der Build erstellt bei Bedarf eine lokale `.venv`, installiert `torch`, `openai-whisper` und PyInstaller aus `requirements.txt`, erzeugt das Icon und buendelt die Python-Abhaengigkeiten in die `.exe`:

```bat
build_windows_exe.bat
```

Die Datei liegt danach unter:

```text
dist\Transcription Windows.exe
```

## Hinweise

- CUDA wird auf Windows automatisch genutzt, wenn eine kompatible NVIDIA-GPU und passende Torch-Installation vorhanden sind.
- Ohne CUDA laeuft die App auf CPU.
- Falls ffmpeg fehlt, kann Whisper Audio- oder Videodateien nicht decodieren.
- Die gebaute `.exe` enthaelt `torch` und `openai-whisper`; Nutzer muessen diese Pakete fuer die EXE nicht separat installieren.
