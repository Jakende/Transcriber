# Transcription Rust für macOS — Work in progress

**Die Rust-Version ist noch Work in progress und noch nicht vollständig
funktionsfähig. Vollständig funktionsfähig sind die bisherigen macOS-
(Swift/Python) und Windows-Anwendungen. Für den täglichen Einsatz die stabilen
Releases verwenden.**

Eigenständige Rust-Neufassung der macOS-Anwendung. Oberfläche, Whisper-Inferenz,
ECAPA-TDNN-Sprecherinferenz, Audiodekodierung, Resampling, Bearbeitung, Speicherung
und Downloads laufen in Rust. Die App startet kein Python, whisper.cpp, ffmpeg,
yt-dlp, Deno oder JavaScript-Programm. macOS-Systemframeworks werden über
Rust-Anbindungen verwendet; die Benutzeroberfläche verwendet egui statt SwiftUI.

## Funktionen

- Dateiauswahl, Drag-and-drop, persistente Warteschlange und Stapelverarbeitung;
  einzelne Dateifehler unterbrechen die übrigen Dateien nicht.
- Whisper Tiny, Small, Medium und Large-v3-Turbo mit Metal oder CPU.
- Deutsch, Englisch oder automatische DE/EN-Erkennung je Audioabschnitt.
- Lokale Sprecherinferenz mit SpeechBrain-ECAPA-TDNN-Gewichten in Candle;
  automatische oder vorgegebene maximale Clusterzahl bis zehn.
- Mehrere Transkriptfenster, Segmentbearbeitung, Suche, Teilen und Verbinden,
  Zeitkorrekturen, Sprecherzuweisung, Umbenennen und Zusammenführen.
- Audiowiedergabe, Segment- und Sprecherhörproben, Schleife und Geschwindigkeit.
- Automatisch gespeicherte Dokumente, Rückgängig/Wiederholen und zwanzig
  Sicherungsversionen pro Dokument.
- Ergebnisbibliothek mit Suche, Archiv, wiederherstellbarem App-Papierkorb und Finder.
- Glossar als Whisper-Kontext und bestätigbare Begriffsvorschläge.
- VTT/SRT-Import, auch mit Stimmenmarkierungen; Import bisheriger Swift-JSON-Dokumente.
- Markdown mit Obsidian-YAML, VTT, SRT, TXT und CSV; Markdown ist die Voreinstellung.
- Kollisionsfreie neue Exporte; bewusst aktualisierte Exporte werden vorher gesichert.
- Podcast-RSS, gespeicherte Feed-Adressen, PodcastIndex-Suche und Episodendownloads;
  Podcast-Metadaten werden im Dokument und Markdown erhalten.
- YouTube-Audiodownload in M4A, optional zusätzliche Videodatei mit Tonspur.
- Abbruch, Fortschritt, Aktivitätsprotokoll, Diagnoseseite und Befehlszeilenzugang.
- Direkte Wiederverwendung vorhandener Whisper-Modelle; optionaler expliziter GGML-Import.

## Vorhandene Modelle automatisch verwenden

Beim ersten Start, über „Diagnose → Erneut suchen“ und vor jedem Download sucht
die App nach vorhandenen Modellen. Sie prüft den eigenen Modellordner, installierte
Transcription-App-Bundles, OpenAI- und Hugging-Face-Zwischenspeicher, typische
whisper.cpp-Verzeichnisse und konfigurierte Suchordner. Zusätzliche Speicherorte
lassen sich unter „Diagnose → Modellordner hinzufügen“ dauerhaft hinterlegen oder
über `TRANSCRIPTION_MODEL_SEARCH_PATHS` angeben.

Unterstützte bestehende Gewichte werden **direkt an ihrem bisherigen Speicherort**
verwendet: Hugging-Face-Safetensors, OpenAI-Whisper-`.pt` sowie Whisper-GGML in
F32/F16 und den üblichen Q4-/Q5-/Q8-Varianten. Die Anpassung an Candle erfolgt im
Arbeitsspeicher. Es wird weder eine zweite Modelldatei angelegt noch das vorhandene
Modell erneut heruntergeladen. Der Standard-App-Build enthält bereits die kleinen
Konfigurations- und Tokenizerdateien aller angebotenen Modelle.

Auch vorhandene ECAPA-Sprechergewichte aus der früheren App oder dem
Hugging-Face-Zwischenspeicher werden wiederverwendet. Python, ffmpeg, Deno,
yt-dlp und whisper-cli werden zu Diagnosezwecken erkannt, sind für diese App aber
nicht mehr erforderlich und werden deshalb weder installiert noch heruntergeladen.

Die Erkennung ist auf bekannte oder ausdrücklich hinzugefügte Verzeichnisse
begrenzt. Ein entferntes externes Laufwerk muss erneut angeschlossen werden.
CTranslate2-/faster-whisper-`model.bin`, GGUF, englischsprachige `.en`-Varianten,
Core-ML-Bundles und beliebige angepasste Modelle werden nicht automatisch als
passendes mehrsprachiges Modell übernommen. Bei unlesbaren Gewichten meldet die
App einen Fehler und überschreibt die vorhandene Datei nicht.

## Gegenüber der bisherigen App geänderte Verfahren

Diese Neufassung ist keine binärkompatible Übernahme der Python-Laufzeit:

- Unterstützte Container/Codecs bestimmt Symphonia: WAV/PCM, MP3, FLAC, AAC/M4A,
  MP4 mit unterstützter Audiospur, ALAC, Ogg/Vorbis, MKV und weitere unterstützte
  Kombinationen. Proprietäre WMV-/WMA-Codecs sowie Opus werden nicht dekodiert.
  Videos ohne unterstützte Audiospur melden einen Dateifehler.
- Downloads behalten ihr Quellformat. Die App enthält keinen MP3-Encoder und
  keinen Video-Multiplexer. Audiospuren können über die Befehlszeile als WAV
  extrahiert werden; zusätzliche YouTube-Videos brauchen eine integrierte Tonspur.
- YouTube wird über eine Rust-Bibliothek mit Boa verarbeitet. Änderungen und
  Zugriffssperren von YouTube können Downloads verhindern; es gibt keinen
  externen yt-dlp-Fallback. Ein erfolgreicher Test garantiert keine dauerhafte Erreichbarkeit.
- Sprecher werden anhand bis zu dreisekündiger ECAPA-Stimmabdrücke pro
  Whisper-Segment gruppiert. Die App verwendet keinen Silero-VAD. Sprecherwechsel
  innerhalb eines Segments müssen im Editor korrigiert werden. Clusterzahlen
  sind keine verlässliche Aussage über die tatsächliche Anzahl der Personen.
- Die Begriffsprüfung arbeitet mit nachvollziehbaren Wortkandidaten und Häufigkeit,
  nicht mit spaCy-Sprachmodellen. Das Glossar steuert einen begrenzten Whisper-Kontext
  und erzwingt keine bestimmte Schreibweise.
- Die neue Bibliothek und Warteschlange liegen getrennt von den Swift-Einstellungen.
  Vorhandene Dokumente können importiert werden. Quelldateien werden nie gelöscht.
- Für den Export verwendet „Teilen“ den Textcursor und zunächst die zeitliche Mitte;
  die Segmentzeiten lassen sich anschließend korrigieren.

## Bauen

macOS 13 oder neuer, Apple Silicon, Xcode Command Line Tools und Rust 1.93.0.

```bash
cd "Transcription Rust"
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
./scripts/build-macos.sh
# Offline-Bundle mit Small und ECAPA:
./scripts/build-macos.sh --bundle-model small
open "dist/Transcription Rust.app"
```

Das Skript erzeugt eine ARM64-App, ein ZIP und eine SHA-256-Datei. Es prüft
Signatur und Systemabhängigkeiten. Die App ist ad-hoc signiert; für eine
Apple-Notarisierung wären Developer-ID-Zugangsdaten erforderlich.

Die Modelle werden beim ersten Gebrauch mit gepinnter Revision heruntergeladen
und mit SHA-256 geprüft. Ein Offline-Bundle enthält das gewählte Modell und
ECAPA. Weitere Modelle lassen sich in der Oberfläche herunterladen.

`TRANSCRIPTION_MODEL_DIR`, `TRANSCRIPTION_DATA_DIR`, `CARGO_TARGET_DIR` und
`TRANSCRIPTION_DIST_DIR` erlauben andere Speicherorte. Große Build-Dateien und
Modelle können dadurch auf einem externen Laufwerk liegen. Eine optionale lokale
`.local-build.env` wird vom Build-Skript gelesen und niemals eingecheckt.

## Befehlszeile

```bash
cargo run --release -- doctor
cargo run --release -- discover-models
cargo run --release -- download-model small --speakers
cargo run --release -- transcribe interview.m4a aufnahme.mp4 --model small \
  --language auto --diarize --speakers 2 --formats md,vtt,srt,txt,csv --output ./Ergebnisse
cargo run --release -- transcribe interview.wav --cpu --no-timecodes --fps 25
cargo run --release -- import untertitel.vtt --audio interview.wav --output ./Import
cargo run --release -- extract film.mp4 tonspur.wav
cargo run --release -- import-ggml ../"Transcription macOS App"/.bundle-cache/resources/models/ggml-small.bin small
```

Der explizite GGML-Import ist optional und erzeugt auf Wunsch eine Safetensors-Kopie.
Im normalen App-Betrieb werden bestehende Gewichte ohne Kopie direkt verwendet.
Der Import verarbeitet F16/F32 sowie unterstützte Q4-/Q5-/Q8-Dateien, ohne das Modell erneut herunterzuladen.
Konfiguration und Tokenizer werden bei Bedarf von Hugging Face geladen. Die
konvertierten Gewichte erhalten eine eigene SHA-256-Provenienz in `import.json`.
Ein vorhandenes Modell wird nicht ersetzt.

Für PodcastIndex `PODCASTINDEX_API_KEY` und `PODCASTINDEX_API_SECRET` in der
Startumgebung setzen; Zugangsdaten werden nicht in den Einstellungen gespeichert.
RSS-Feeds funktionieren ohne diese Zugangsdaten.

## Daten und Wiederherstellung

`doctor` und die Diagnoseseite zeigen die tatsächlich verwendeten Pfade.
`settings.json` enthält Warteschlange, Glossar und Einstellungen; `library.json`
die Bibliothek. Dokumente sind lesbares JSON unter `documents/`.
`backups/<Dokument-ID>/` enthält Sicherungsversionen. `Papierkorb/` enthält
verschobene Bibliotheksdokumente; sie können mit „Importieren“ wieder aufgenommen
werden. `export-backups/` sichert ausdrücklich aktualisierte Exporte.

## Prüfung

`tests/core.rs` prüft echte WAV-Dekodierung und Resampling, Abbruch, Zeitformate,
UTF-8-Bearbeitung, Rückgängig/Wiederholen, VTT/SRT, CSV, YAML, Dateikollisionen,
atomare Speicherung, Whisper-Melfilter gegen OpenAI-Referenzdaten sowie
Sprechermerkmale und Clustering. Der lokale Abnahmebericht dokumentiert zusätzlich
Inferenz- und App-Prüfungen, soweit ausgeführt.

Die bisherigen macOS-, Swift- und Windows-Ordner bleiben eigenständig.
Lizenzhinweise: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
