# Funktionsabgleich zur bisherigen macOS-App

Rust 0.2.0 bleibt **Work in progress**. Die bisherigen macOS- und Windows-Apps
bleiben die vollständig unterstützten Fassungen. Die neue Laufzeit besteht aus
Rust und macOS-Systemframeworks; sie startet keine Python-, ffmpeg-, yt-dlp-,
Deno- oder whisper.cpp-Hilfsprogramme.

Referenz: README, Swift- und Python-Implementierung der bisherigen macOS-App.

| Bereich | Umsetzung und Nachweis |
| --- | --- |
| Dateien, Warteschlange, Stapel, Fehlerisolation | Persistente Warteschlange; Baseline-Bedienprüfung und erneute echte Inferenz; einzelne Dateifehler werden isoliert |
| Whisper, CPU/Metal, vorhandene Modelle | Tiny/Small/Medium/Turbo; direkte Nutzung vorhandener PT-, GGML- und Safetensors-Gewichte; erneute Inferenz mit Tiny/CPU sowie Small, Medium und Turbo/Metal und jeweils fünf Exportformaten bestanden |
| Deutsch/Englisch/automatisch | Automatische Erkennung an Pausen in kürzeren Abschnittsgruppen; erneuter zweistimmiger DE/EN-Test erzeugt beide Sprachen |
| Sprecher | Silero-VAD mit korrigiertem Streaming-Kontext, überlappende 1,5-s-Fenster, ECAPA, hierarchisches Cosinus-Clustering; echte zweistimmige Aufnahme mit zwei Stimmen und acht Bereichen verarbeitet |
| Sprecheroptionen | Automatisch, genau zwei, 2–4, 4–6, 5–8, 6–10; vier Trennungsstärken; Grenzfälle und Fensterbildung getestet |
| Sprecherwechsel und Bearbeitung | Wechsel teilen Sätze an Wortgrenzen; Zeitanteil schätzt die Wortposition. Namen, Zuweisen und Zusammenführen erhalten Sprecherbereiche und Metadaten |
| Editor und Wiedergabe | Mehrere Fenster, Textsuche, Segment-/Sprecherhörproben, Zeitkorrektur, Schleife und Geschwindigkeit; im neuen Paket einschließlich Schleife, 1,25-facher Geschwindigkeit, Speichern, Neustart und Exportaktualisierung geprüft |
| Teilen/Verbinden/Löschen, Undo/Redo | Wortgrenzen, Unicode, Stimmübernahme beim Verbinden und Undo/Redo getestet |
| Speicherung und Wiederherstellung | Atomare Speicherung, 20 Sicherungen, automatisches Öffnen der neuesten gültigen Sicherung bei beschädigter Hauptdatei; Original wird beim Lesen erhalten; Regressionstest |
| Bibliothek, Archiv, Papierkorb | Suche/Archiv/Finder; App-Papierkorb über Import wiederherstellbar; Speichern/Archiv/Neustart/Papierkorb ohne Änderung der Medien/Exporte getestet |
| RSS und Podcast-Stapel | Mehrfachauswahl im echten NPR-Feed in der App geprüft; zwei lokale Testfolgen im signierten Paket heruntergeladen und automatisch nacheinander mit fünf Formaten transkribiert |
| Podcast-Metadaten | RSS/iTunes: Sendung, Folge, Autoren, Sprache, Datum, GUID, Folge/Staffel/Typ, Dauer, Kennzeichnung, Bilder, Kategorien, Beschreibungen; Erhalt in JSON/Markdown getestet |
| PodcastIndex | Native Schlüsselbundspeicherung, Umgebungsvariablen und Wiederverwendung des bisherigen Zugangs; Live-Suche erfolgreich ohne erneute Eingabe |
| YouTube | Neue Besuchersitzung und VISIONOS-Playeranfrage in Rust; MP3-Kodierung und MP4-Multiplexing ohne Hilfsprozess; echter Download von „Me at the zoo“ als MP3 und MP4 mit Bild/Ton, unabhängig vollständig dekodiert |
| Audio/Video | PCM/WAV, AIFF, FLAC, MP3, AAC, ALAC, Vorbis, Opus, MP4/MOV/MKV/WebM, AVI, FLV, MPEG-TS, MPEG-1/2-PS und WMA-v1/v2-Basis; echte komprimierte Sprach-Fixtures mit Dauer- und Signalkorrelationstest |
| Begriffsprüfung | Lokales mehrsprachiges DistilBERT erkennt Personen/Organisationen/Orte; echte DE/EN-Inferenz mit Namensgruppen und Häufigkeiten geprüft; Ergebnisse werden gespeichert |
| Glossar und Ersetzungen | Whisper-Kontext; Bestätigung und wortweise Ersetzung, Unicode-Grenzen und literale Ersetzungstexte getestet |
| Import/Export | Swift-JSON einschließlich zusätzlicher Metadaten; VTT/SRT-Stimmen und Markup; Markdown/SRT/VTT/TXT/CSV einschließlich Obsidian-YAML, Sprecher- und Podcast-Metadaten |
| Abbruch und Fortschritt | Netzwerkabbruch auch bei blockierter Antwort getestet; Abbruchprüfungen in Dekodierung, Resampling, MP3, Modellen, VAD, Sprecher-, Sprach- und YouTube-Verarbeitung |
| Paket, CI und Veröffentlichung | Signiertes ARM64-Paket lokal geprüft; GitHub-Actions-Erfolg ist Voraussetzung für Merge und Veröffentlichung. Details: VALIDATION-0.2.md |

## Grenzen der Rust-Verfahren

Funktionsübernahme bedeutet keine identischen neuronalen Ergebnisse. Die
Begriffsprüfung verwendet DistilBERT statt spaCy, das Clustering ist hierarchisch
statt des bisherigen Spectral-Clustering-Pfads. Satzinterne Wortzeiten werden
angenähert. Diese Unterschiede erfordern weiterhin redaktionelle Kontrolle.

ASF unterstützt derzeit unverschlüsselte WMA-v1/v2-Basisspuren mit ein oder zwei
Kanälen und festen ASF-Paketgrößen. WMA Pro/Lossless und die gesamte Menge
seltener ffmpeg-Codecs sind nicht portiert. Beschädigte oder nicht normgerechte
Streams können abgewiesen werden. YouTube kann anmeldepflichtige Videos und
Zugriffe sperren oder sein Protokoll ändern.

Whisper- und ECAPA-Gewichte werden am vorhandenen Speicherort verwendet. Silero
ist eingebettet. Das zusätzliche Sprachmodell benötigt einmalig ca. 539 MB und
arbeitet danach offline. Das normale ZIP enthält Tokenizer/Konfigurationen,
keine großen Whisper-/ECAPA-/DistilBERT-Gewichte.

Die Regulärtests benötigen weder Netzwerk noch Modelle. Der gekennzeichnete
neuronale Test wird zur lokalen Release-Abnahme ausdrücklich zusätzlich gestartet.
