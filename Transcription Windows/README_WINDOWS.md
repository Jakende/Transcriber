# Transcription Windows

Windows-Desktop-App für lokale Audio- und Video-Transkription mit dem Funktionsumfang der nativen macOS-App.

## Funktionen

- Persistente Warteschlange per Dateiauswahl oder Drag-and-drop
- Direkter Import ausgewählter Podcast-Folgen aus RSS und PodcastIndex
- Download einzelner YouTube-Videos als MP3, optional zusätzlich als Video
- Deutsch, Englisch und automatische Erkennung gemischtsprachiger Aufnahmen
- Whisper Small, Medium und Large-v3-Turbo
- Optionale Sprechererkennung mit automatischer oder begrenzter Sprecherzahl
- Sprecher-Hörproben, frei benennbare Sprecher:innen und Zusammenführen falsch getrennter Stimmen
- Transkript-Editor mit Suche, Teilen, Verbinden und Löschen von Segmenten
- Audio-Wiedergabe mit Schleife, Sprüngen und variabler Geschwindigkeit
- Rückgängig/Wiederholen, automatische Sicherungsversionen und wiederhergestellte Ergebnisbibliothek
- Ergebnis-Suche, Archiv und Papierkorb-Aktion für interne Bearbeitungsstände
- Persistentes Wörterbuch als Whisper-Kontext und zweisprachige Begriffsprüfung mit spaCy
- Abschnittsweise DE/EN-Kennzeichnung
- Import vorhandener VTT-Dateien mit optionaler Mediendatei
- Markdown-, VTT-, SRT-, TXT- und CSV-Ausgabe ohne stilles Überschreiben
- Obsidian-kompatibles YAML-Frontmatter einschließlich Podcast-Metadaten
- Strukturierter Fortschritt, Abbruchanforderung und isolierte Fehler pro Datei
- Systembericht für Laufzeit, Modelle und gebündelte Werkzeuge

## Installation

Für normale Nutzer:innen: `Transcription-Windows-Setup-v2.1.exe` aus dem GitHub-Release laden und ausführen. Der Installer richtet die App im Startmenü ein und kann optional eine Desktop-Verknüpfung anlegen.

Für lokale Entwicklung werden Windows 10 oder neuer sowie Python 3.10+ benötigt:

```bat
install_windows.bat
run_windows_app.bat
```

Die Release-App bündelt Python-Abhängigkeiten, FFmpeg, FFprobe, FFplay, Deno und yt-dlp. Whisper- und SpeechBrain-Modellgewichte werden bei ihrer ersten Nutzung nach `%LOCALAPPDATA%\Transcription Windows\models` geladen und anschließend lokal wiederverwendet. Der erste Modellstart benötigt daher eine Internetverbindung.

## Bedienung

1. Dateien hinzufügen, per Drag-and-drop ablegen, VTT importieren oder Medien laden.
2. Sprache, Whisper-Modell, Sprecherbereich, Trennung und Ausgabeformate einstellen.
3. Optional einen gemeinsamen Zielordner wählen.
4. `Transkription starten` wählen.
5. Fertige Ergebnisse über `Bearbeiten` nachbearbeiten oder neu exportieren.

Bei Videodateien wird gezielt die erste Audiospur verarbeitet. Dateien ohne Audiospur werden übersprungen und im Aktivitätsprotokoll ausgewiesen.

## EXE und Installer bauen

```bat
build_windows_exe.bat
build_windows_installer.bat
```

Der EXE-Build installiert die gepinnten Abhängigkeiten, erzeugt das App-Icon, lädt und prüft FFmpeg sowie Deno, bündelt die gemeinsame Transkriptionslogik und führt anschließend einen Smoke-Test der fertigen EXE aus. Der Installer-Build verwendet Inno Setup.

Die lokalen Standardausgaben sind:

```text
dist\Transcription Windows.exe
dist\Transcription-Windows-Setup-v2.1.exe
```

CUDA wird automatisch verwendet, wenn eine kompatible NVIDIA-GPU und die mitgelieferte Torch-Version sie nutzen können; andernfalls läuft die Transkription auf der CPU.

## Tests

Vom Repository-Stamm aus:

```bat
python -m py_compile "Transcription Windows\transcription_windows_app.py" "Transcription Windows\windows_services.py"
python -m unittest tests.test_windows_services tests.test_windows_safe_streams
```

Lizenz- und Quellenhinweise stehen in `..\NOTICE` und `..\THIRD-PARTY-LICENSES.md`.
