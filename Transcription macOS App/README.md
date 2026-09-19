# Transcription macOS App

Native SwiftUI-App für vollständig lokale Audio- und Video-Transkription auf Apple Silicon. Die App verwendet `whisper.cpp` mit Metal und kann Stimmen mit Silero VAD sowie SpeechBrain ECAPA-TDNN unterscheiden.

## Funktionen

- Stapelverarbeitung per Dateiauswahl oder Drag-and-drop
- Medien-Dialog für ausgewählte Podcast-Folgen aus RSS/PodcastIndex und einzelne YouTube-Videos
- YouTube-Audio als MP3 mit optionaler zusätzlicher Videodatei und optionalem Transkriptionsstart
- Direkte Tonspurextraktion aus MP4, MOV, M4V, MKV, WebM, AVI, MPEG/MPG, MTS/M2TS, TS, VOB, FLV, 3GP und WMV
- Deutsch, Englisch und automatische Erkennung für gemischtsprachige Aufnahmen
- Whisper Small, Medium und Large-v3-Turbo
- Optionale Sprechererkennung mit automatischer oder begrenzter Sprecherzahl
- Sprecher-Hörproben, frei benennbare Sprecher und nachträgliches Zusammenführen falsch getrennter Stimmen
- Mehrfenster-Transkript-Editor mit Audio-Wiedergabe, Schleife und Geschwindigkeit
- Automatisch gespeicherte Bearbeitungsstände und wiederhergestellte Ergebnisliste
- Rückgängig/Wiederholen und automatische Sicherungsversionen für die Nachbearbeitung
- Ergebnisbibliothek mit Suche, Archiv und Papierkorb-Aktion
- Segment-Suche sowie Teilen und Verbinden von Segmenten
- Persistente Dateiwarteschlange und bestätigte Glossarbegriffe als Whisper-Kontext
- Abschnittsweise DE/EN-Kennzeichnung im gemischtsprachigen Editor
- Zweisprachige Begriffsprüfung mit spaCy
- Import vorhandener VTT-Dateien mit optionaler Audiodatei
- Auswählbare Markdown-, VTT-, SRT-, TXT- und CSV-Ausgabe
- Obsidian-kompatibles YAML-Frontmatter
- Abbruch, strukturierter Fortschritt und isolierte Fehler pro Datei

Markdown ist standardmäßig das einzige aktivierte Ausgabeformat. Vorhandene Dateien werden nicht still überschrieben; neue Transkripte erhalten bei Namenskollisionen einen fortlaufenden Suffix.

Bei Videodateien verarbeitet die App gezielt die erste Audiospur. Dateien ohne Audiospur werden übersprungen und mit einer verständlichen Fehlermeldung im Aktivitätsprotokoll ausgewiesen.

## Entwicklungsbetrieb

Voraussetzungen:

- macOS 13 oder neuer auf Apple Silicon
- Swift 5.9 beziehungsweise Xcode Command Line Tools
- Python mit `torch`, `soundfile`, SpeechBrain, Silero VAD und scikit-learn
- `whisper-cli`, ffmpeg und passende GGML-Modelle
- Für YouTube-Downloads: yt-dlp und Deno; im vollständigen App-Bundle sind beide enthalten

Syntaxprüfungen und Tests:

```bash
python3 -m py_compile Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py Sources/TranscriptionMacOSApp/Resources/transcription_backend/*.py
python3 -m unittest ../tests/test_macos_transcription_backend.py
swift test
```

Lokalen Entwicklungsbuild erstellen:

```bash
./script/build_and_run.sh --no-run
```

Sind bereits vollständige Offline-Ressourcen in `.bundle-cache` vorhanden, werden sie auch in den lokalen Build übernommen. Andernfalls kennzeichnet das Skript das Ergebnis ausdrücklich als Entwicklungsbundle ohne Transkriptionslaufzeit. Veröffentlichbare Builds müssen mit `--bundle` erstellt werden.

## Vollständig gebündelte App

Der Offline-Build lädt reproduzierbar eine ARM64-CPython-Laufzeit, gepinnte Python-Pakete einschließlich yt-dlp, Deno, `whisper.cpp`, ffmpeg, drei Whisper-Modelle, ECAPA-TDNN und beide spaCy-Sprachmodelle. Dafür werden mehrere Gigabyte Speicher und beim ersten Build eine schnelle Internetverbindung benötigt.

```bash
./script/build_and_run.sh --bundle --no-run
```

Jeder vollständige Build wird anschließend mit `script/verify_app_bundle.sh` auf `whisper-cli`, ffmpeg, yt-dlp, Deno, Python, beide spaCy-Modelle, das Sprechermodell und alle Whisper-Modelle geprüft. Die GitHub-Release-Pipeline führt dieselbe Prüfung vor dem Verpacken aus.

Das App-Bundle liegt danach unter:

```text
dist/Transcription macOS.app
```

Die fertige App benötigt für Transkription, Sprechererkennung und Begriffsprüfung keine Netzwerkverbindung. Der Build ist ad-hoc signiert, aber ohne bereitgestellte Apple-Developer-Zugangsdaten nicht notarisiert.

Lizenz- und Quellenhinweise stehen in `../NOTICE` und `../THIRD-PARTY-LICENSES.md` und werden in das App-Bundle kopiert.
