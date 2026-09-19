# Drittanbieter-Komponenten der Transkriptions-Apps

Die macOS-App verarbeitet alle Medien lokal; die Windows-App lädt Whisper- und SpeechBrain-Gewichte beim ersten Einsatz in ihren lokalen Modellcache. Je nach Plattform werden folgende Komponenten und Modellgewichte eingebettet oder verwendet:

- [LocalTranscript](https://github.com/BenPohlBasel/LocalTranscript), MIT; Grundlage der adaptierten Silero-/ECAPA-Diarisierung.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp), MIT.
- [OpenAI Whisper model weights](https://github.com/openai/whisper), MIT.
- [SpeechBrain](https://github.com/speechbrain/speechbrain) und [ECAPA-TDNN speaker model](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb), Apache-2.0.
- [Silero VAD](https://github.com/snakers4/silero-vad), MIT.
- [PyTorch](https://github.com/pytorch/pytorch), BSD-3-Clause.
- [scikit-learn](https://github.com/scikit-learn/scikit-learn), BSD-3-Clause.
- [spaCy](https://github.com/explosion/spaCy) sowie `de_core_news_sm` und `en_core_web_sm`, MIT.
- [python-build-standalone](https://github.com/astral-sh/python-build-standalone), verschiedene kompatible Open-Source-Lizenzen gemäß Upstream-Manifest.
- [ffmpeg-static](https://github.com/eugeneware/ffmpeg-static), GPL-3.0-or-later; ffmpeg wird als eigenständiger Prozess ausgeführt. Der zugehörige Quellcode und die Buildinformationen sind im verlinkten Upstream-Projekt verfügbar.
- [yt-dlp](https://github.com/yt-dlp/yt-dlp), Unlicense; zusätzliche gebündelte Komponenten stehen unter ISC- und MIT-Lizenzen gemäß Upstream-Manifest.
- [yt-dlp-ejs](https://github.com/yt-dlp/ejs), Unlicense; die enthaltenen Bibliotheken `meriyah` und `astring` stehen unter ISC beziehungsweise MIT.
- [Deno](https://github.com/denoland/deno), MIT; wird als eingeschränkte JavaScript-Laufzeit für die YouTube-Extraktion verwendet.
- [OpenAI Whisper](https://github.com/openai/whisper), MIT; Python-Transkriptionslaufzeit der Windows-App.
- [TkinterDnD2](https://github.com/Eliav2/tkinterdnd2), MIT; Drag-and-drop unter Windows.
- [keyring](https://github.com/jaraco/keyring), MIT; sichere Ablage der PodcastIndex-Zugangsdaten unter Windows.
- [Send2Trash](https://github.com/arsenetar/send2trash), BSD-3-Clause; Papierkorb-Aktionen unter Windows.
- [Gyan FFmpeg Builds](https://www.gyan.dev/ffmpeg/builds/), GPL; Windows-Builds von FFmpeg, FFprobe und FFplay.

Vor einer öffentlichen Weitergabe müssen diese Hinweise zusammen mit den vollständigen Lizenztexten des konkret erzeugten Bundles ausgeliefert und die jeweiligen Weitergabepflichten geprüft werden.
