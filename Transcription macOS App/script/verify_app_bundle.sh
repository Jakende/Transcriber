#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
  echo "Verwendung: $0 /Pfad/zu/Transcription macOS.app" >&2
  exit 2
fi

APP_DIR="$1"
RESOURCE_DIR="${APP_DIR}/Contents/Resources"
REQUIRED_PATHS=(
  "${APP_DIR}/Contents/MacOS/TranscriptionMacOSApp"
  "${RESOURCE_DIR}/transcribe_bulk.py"
  "${RESOURCE_DIR}/bin/whisper-cli"
  "${RESOURCE_DIR}/bin/ffmpeg"
  "${RESOURCE_DIR}/bin/deno"
  "${RESOURCE_DIR}/python-runtime/bin/python3"
  "${RESOURCE_DIR}/models/ggml-small.bin"
  "${RESOURCE_DIR}/models/ggml-medium.bin"
  "${RESOURCE_DIR}/models/ggml-large-v3-turbo.bin"
  "${RESOURCE_DIR}/models/speaker/spkrec-ecapa-voxceleb/hyperparams.yaml"
)

missing=0
for path in "${REQUIRED_PATHS[@]}"; do
  if [[ ! -f "${path}" ]]; then
    echo "Fehlt im App-Bundle: ${path}" >&2
    missing=1
  fi
done
if [[ "${missing}" -ne 0 ]]; then
  exit 1
fi

if [[ ! -x "${RESOURCE_DIR}/bin/whisper-cli" \
   || ! -x "${RESOURCE_DIR}/bin/ffmpeg" \
   || ! -x "${RESOURCE_DIR}/bin/deno" \
   || ! -x "${RESOURCE_DIR}/python-runtime/bin/python3" ]]; then
  echo "Mindestens eine gebündelte Laufzeitdatei ist nicht ausführbar." >&2
  exit 1
fi

DYLD_LIBRARY_PATH="${RESOURCE_DIR}/lib" "${RESOURCE_DIR}/bin/whisper-cli" --version >/dev/null 2>&1
"${RESOURCE_DIR}/bin/ffmpeg" -version >/dev/null
"${RESOURCE_DIR}/bin/deno" --version >/dev/null
PYTHONDONTWRITEBYTECODE=1 "${RESOURCE_DIR}/python-runtime/bin/python3" -m yt_dlp --version >/dev/null
HF_HUB_OFFLINE=1 PYTHONDONTWRITEBYTECODE=1 "${RESOURCE_DIR}/python-runtime/bin/python3" - <<'PY'
import numpy
import soundfile
import spacy
import torch

spacy.load("de_core_news_sm")
spacy.load("en_core_web_sm")
PY

echo "App-Bundle vollständig geprüft: ${APP_DIR}"
