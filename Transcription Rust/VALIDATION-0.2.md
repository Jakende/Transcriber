# Abnahmeprotokoll Rust 0.2.0 (WIP)

Lokale Prüfungen am 11. November 2026 auf Apple Silicon/macOS. Technische
Prüfungen ersetzen keine redaktionelle Kontrolle neuronaler Transkripte.
Die Einschränkungen in PARITY.md und README.md gelten weiterhin.

## Automatische Prüfungen

- Rust 1.95.0: `cargo fmt --all --check` und `cargo clippy --locked --all-targets -- -D warnings` bestanden.
- `cargo test --locked --all-targets`: 31 Tests bestanden, ein neuronaler Test regulär übersprungen.
- `cargo test multilingual_neural_terms -- --ignored`: neuronaler Test zusätzlich bestanden; gepinntes mehrsprachiges DistilBERT mit SHA-256-Prüfung.
- Sprach-Fixtures: AVI/MP3, FLV/MP3, MPEG-TS/AAC, MPEG-1-PS/MP2, ASF/WMA-v2, Ogg/Opus, WebM/Opus und M4A/AAC dekodiert. Dauer und Signalkorrelation > 0,90 zum Original geprüft; zeitlicher Codec-Vorlauf berücksichtigt.
- MP4-Multiplexing bewahrt Ton und Bild und ersetzt keine vorhandene Zieldatei. MP3 wurde mit einem unabhängigen Decoder gelesen.
- Abbruch bei blockiertem HTTP-Server, unvollständige Downloads, Dateikollisionen, Metadatenerhalt, Untertitel, YAML, UTF-8, Rückgängig/Wiederholen und Wortteilung bestanden.
- Bibliothek: Speichern, Sicherung, Archivierung, erneutes Öffnen, Wiederherstellung einer beschädigten Hauptdatei ohne Überschreiben beim Lesen und wiederherstellbarer Papierkorb bestanden; Medien und Exporte bleiben erhalten.
- Die vorgeschriebenen Python-Syntaxprüfungen der unveränderten bisherigen Plattformdateien bestanden.

## Echte Inferenz und Netzdienste

| Prüfung | Beobachtung |
| --- | --- |
| Tiny/CPU | Öffentliche viersekündige JFK-Sprachaufnahme transkribiert; fünf Exporte erzeugt |
| Small/Metal | Gleiche Aufnahme mit bestehenden OpenAI-PT-Gewichten verarbeitet; fünf Exporte |
| Medium/Metal | Gleiche Aufnahme mit vorhandenen PT-Gewichten verarbeitet; fünf Exporte |
| Turbo/Metal | Vorhandene GGML-Gewichte verwendet; fünf Exporte |
| DE/EN, zwei Stimmen | Kombinierte deutsche/englische Sprachaufnahme mit Silero/ECAPA, genau zwei Stimmen und acht Sprecherbereichen verarbeitet; beide Sprachannotationen vorhanden |
| NPR-RSS | Echter Feed eingelesen und Mehrfachauswahl in der App bedient |
| PodcastIndex | Echte Suche erfolgreich mit wiederverwendetem Zugang, ohne erneute Eingabe oder Ausgabe der Zugangsdaten |
| YouTube | `jNQXAC9IVRw` („Me at the zoo“) in Rust als MP3 (~19,10 s) und MP4 (~19,06 s, H.264 + Stereo-AAC) geladen/erzeugt; beide Dateien unabhängig vollständig dekodiert |
| Begriffsprüfung | Personen, Orte und Organisationen mit DE/EN-Text und Mehrwortnamen/Häufigkeiten geprüft; danach offline aus lokalem Modell verwendbar |

ffmpeg und die bisherige App wurden ausschließlich als Entwicklungsreferenz
verwendet. Die ausgelieferte Rust-App startet diese Programme nicht.

## Bedienprüfung im signierten App-Paket

Die Prüfung verwendete eine isolierte Testbibliothek und öffentliche/synthetische
Testmedien. Bestehende Benutzertranskripte wurden nicht bearbeitet.

- RSS mit zwei Folgen: Mehrfachauswahl, Download, automatische Stapeltranskription, beide Bibliothekseinträge und jeweils alle fünf Exporte beobachtet.
- Transkript geöffnet, Audio geladen, Segment-/Sprecherhörprobe, Schleife und Geschwindigkeit 1,25 bedient.
- Wortteilung: zwei Segmente wurden zu drei; Rückgängig stellte zwei wieder her; Wiederholen und Verbinden bedient.
- Lokale Sprachanalyse erzeugte Mehrwortvorschläge. „New York“ zu „New York City“ korrigiert; bestätigte Form ins Glossar aufgenommen; Exporte mit Sicherung aktualisiert.
- Sprecher umbenannt, zweitem Segment eine andere Stimme zugewiesen und Stimmen zusammengeführt; Sprecherbereiche/Zuordnungen erhalten.
- Bibliothekseintrag archiviert, im Archiv wiedergefunden und wiederhergestellt.
- Testdokument in den App-Papierkorb verschoben und über den nativen Importdialog wiederhergestellt.
- Nach App-Neustart waren Textkorrekturen, Sprecherzuordnung und Begriffsergebnisse vorhanden.
- Zwei bei der Bedienprüfung gefundene Fehler behoben und im neu gebauten Paket nachgeprüft: leere/gestoppte Wiedergabe zeigt „Wiedergeben“ und startet den gewählten Abschnitt; bestätigte Ersetzungen werden nach einmaliger Anwendung geleert. Stopp setzt die Anzeige auf null.

## Paket und Veröffentlichung

`build-macos.sh` erstellt die ARM64-App mit Konfigurationen und Tokenizern für
alle vier Whisper-Modelle. `verify-macos.sh` prüft Info.plist, strikte ad-hoc
Signatur und ausschließlich macOS-Systembibliotheken. Große Modellgewichte
bleiben an ihrem vorhandenen Ort oder werden bei Bedarf geladen.

Der lokale Paketbau und diese technischen Prüfungen bestanden. Für die
Veröffentlichung werden zusätzlich die GitHub-Actions-Prüfungen des endgültigen
Commits und des Main-Merges abgewartet. Der zugehörige erfolgreiche Lauf wird
in den GitHub-Release-Notizen verlinkt. Dieses Dokument behauptet keine
Apple-Notarisierung oder vollständige Abdeckung sämtlicher ffmpeg-Codecs.
