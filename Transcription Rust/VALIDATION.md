# Lokale Abnahme der Rust-Neufassung

Geprüft auf einem Apple-Silicon-Mac mit Rust 1.93.0. Die Prüfung ersetzt keine
Kompatibilitätsprüfung auf allen unterstützten macOS-Versionen.

## Automatische Prüfungen

- `cargo test --locked --all-targets`: 15 Tests bestanden.
- `cargo clippy --locked --all-targets -- -D warnings`: ohne Warnungen.
- `cargo fmt --all --check`: bestanden.
- Python-Syntaxprüfungen der bisherigen Plattform-Einstiegspunkte: bestanden;
  die bisherigen Workflows wurden nicht geändert.
- Beide Shell-Buildskripte mit `bash -n` geprüft.

Die Tests decken WAV-Dekodierung/Resampling, Abbruch, YAML, CSV, VTT/SRT,
Unicode-Bearbeitung, Segmentoperationen, Rückgängig/Wiederholen, atomare Speicherung,
Dateikollisionen, Referenz-Melfilter, Clustering und nicht schreibende Modellsuche ab.

## Echte Inferenz und Audioformate

| Prüfung | Beobachtung |
| --- | --- |
| Whisper Tiny, CPU, automatische DE/EN-Erkennung | Kurze deutsche Aufnahme erfolgreich transkribiert; Markdown ohne Zeitcodes erstellt. |
| Whisper Small, Metal, vorhandene OpenAI-`.pt`-Datei | Ohne erneuten Modelldownload erfolgreich transkribiert. |
| Whisper Large-v3-Turbo, Metal, vorhandene GGML-Datei | Direkt aus dem installierten bisherigen App-Bundle gelesen; zwei zeitlich gültige Segmente erzeugt. |
| ECAPA-TDNN, Candle/Metal | 192-dimensionale normalisierte Stimmabdrücke erzeugt; Sprecher zugewiesen. |
| ECAPA gegenüber unabhängig berechneter PyTorch-Referenz | Kosinusähnlichkeit 0,999999999997; maximale absolute Komponentenabweichung etwa 6,3 × 10⁻⁷. |
| WAV, MP3, AAC/M4A und MP4 mit AAC-Tonspur | Mit Rust dekodiert und als WAV extrahiert. ffmpeg diente ausschließlich zur Herstellung der Testdateien. |
| MP4 ohne Audio | Verständlicher Dateifehler; keine WAV-Ausgabe erstellt. |
| Ungültige Datei vor gültiger Aufnahme im Stapel | Fehler isoliert; die nachfolgende Aufnahme wurde trotzdem exportiert. |
| Markdown, VTT, SRT, TXT und CSV | Mit echter Inferenz erzeugt; Zeitbereiche innerhalb der Aufnahme geprüft. |

Der numerische ECAPA-Vergleich prüft die Implementierung des Modells. Er ist kein
Qualitätsbenchmark für Sprechertrennung in realen Interviews oder Überlappungen.
Die Referenz-Python-Umgebung, Testmedien und Stimmabdrücke gehören nicht zur App
und wurden nicht in Git aufgenommen.

## Manuelle Prüfung der gebauten App

- App-Bundle gestartet; automatische Erkennung vorhandener Tiny-, Small-, Medium-
  und Turbo-Modelle beobachtet. OpenAI-Zwischenspeicher und vorherige App-Bundles
  wurden gefunden. Die Modellanzeige bestätigt die direkte Wiederverwendung.
- Nativer Dateidialog und Übernahme in die Warteschlange geprüft.
- Small-Transkription mit Sprechererkennung und fünf Ausgabeformaten in der App
  abgeschlossen; Ergebnisbibliothek und dauerhaft gespeicherte Exporte geprüft.
- Eigenes Editorfenster, Unicode-Textbearbeitung, Rückgängig, Segmentteilung,
  automatisches Speichern und angelegte Sicherungsversionen geprüft.
- Audio laden und Segmentwiedergabe geprüft; die Wiedergabeposition läuft weiter.
- Gestaltete Oberfläche im Hell- und Dunkelmodus sowie bei 880 × 620 Punkten
  kontrolliert; Scrollbereiche erhalten den Zugriff auf alle Einstellungen.

## Paketprüfung und Grenzen

Das Buildskript erzeugt ein ARM64-App-Bundle und ein ZIP mit SHA-256-Prüfsumme.
`verify-macos.sh` prüft die ad-hoc Signatur, Info.plist und dynamische
Systemabhängigkeiten. Die App enthält keine Python-Dateien, externen
Transkriptionsprogramme oder mitgelieferten dynamischen Bibliotheken.

Der Standardbuild enthält Modellkonfigurationen und Tokenizer, keine zweite
Kopie bereits vorhandener großer Modellgewichte. Ohne vorhandene passende
Gewichte lädt die App das gewählte Modell beim ersten Gebrauch herunter.

RSS/PodcastIndex und YouTube sind implementiert, aber nicht mit einem vollständigen
Live-Download-End-to-End-Test abgenommen. Insbesondere kann YouTube automatisierte
Zugriffe sperren. Abweichungen von der Swift/Python-Version, unterstützte
Modellformate und Audio-Codecs sind im README beschrieben. Apple-Notarisierung
wurde mangels Developer-ID-Zugangsdaten nicht durchgeführt.
