# Drittanbieter-Komponenten der macOS-App

Die gebündelte macOS-App verarbeitet alle Medien lokal. Die folgenden Komponenten und Modellgewichte werden beim Offline-Build eingebettet:

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

Vor einer öffentlichen Weitergabe müssen diese Hinweise zusammen mit den vollständigen Lizenztexten des konkret erzeugten Bundles ausgeliefert und die jeweiligen Weitergabepflichten geprüft werden.
