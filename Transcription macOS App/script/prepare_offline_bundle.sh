#!/usr/bin/env bash
set -euo pipefail

# Der eingebettete Python-Interpreter darf den reproduzierbaren Ressourcen-Cache
# nicht durch Laufzeit-Bytecode vergrößern oder später die App-Signatur ändern.
export PYTHONDONTWRITEBYTECODE=1

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${ROOT_DIR}/.bundle-cache"
DOWNLOAD_DIR="${CACHE_DIR}/downloads"
RESOURCE_DIR="${CACHE_DIR}/resources"
RUNTIME_DIR="${RESOURCE_DIR}/python-runtime"
BIN_DIR="${RESOURCE_DIR}/bin"
LIB_DIR="${RESOURCE_DIR}/lib"
MODEL_DIR="${RESOURCE_DIR}/models"
SPEAKER_MODEL_DIR="${MODEL_DIR}/speaker/spkrec-ecapa-voxceleb"

PYTHON_ARCHIVE="cpython-3.13.14+20260728-aarch64-apple-darwin-install_only.tar.gz"
PYTHON_URL="https://github.com/astral-sh/python-build-standalone/releases/download/20260728/cpython-3.13.14%2B20260728-aarch64-apple-darwin-install_only.tar.gz"
PYTHON_SHA256="a48399e6ddbb9ccf215eb5afea8f40eca289c808f99faa5d38f35f575a6f1784"
WHISPER_TAG="v1.9.1"
WHISPER_COMMIT="f049fff95a089aa9969deb009cdd4892b3e74916"
FFMPEG_URL="https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz"
FFMPEG_SHA256="8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa"
DENO_VERSION="2.9.6"
DENO_ARCHIVE="deno-aarch64-apple-darwin-${DENO_VERSION}.zip"
DENO_URL="https://github.com/denoland/deno/releases/download/v${DENO_VERSION}/deno-aarch64-apple-darwin.zip"
DENO_SHA256="213a2f304f04d3c9cb5220669afad138f60a5aab1fe80962abdeb8f35807a472"
MODEL_REVISION="5359861c739e955e79d9a303bcbc70fb988958b1"

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Der Offline-Build muss auf Apple Silicon ausgeführt werden." >&2
  exit 1
fi

mkdir -p "${DOWNLOAD_DIR}" "${RESOURCE_DIR}" "${BIN_DIR}" "${LIB_DIR}" "${MODEL_DIR}"

download_verified() {
  local url="$1"
  local destination="$2"
  local expected="$3"
  if [[ ! -f "${destination}" ]] || [[ "$(shasum -a 256 "${destination}" | awk '{print $1}')" != "${expected}" ]]; then
    curl --fail --location --retry 3 --output "${destination}.part" "${url}"
    if [[ "$(shasum -a 256 "${destination}.part" | awk '{print $1}')" != "${expected}" ]]; then
      echo "Prüfsummenfehler: ${destination}" >&2
      rm -f "${destination}.part"
      exit 1
    fi
    mv "${destination}.part" "${destination}"
  fi
}

download_model() {
  local name="$1"
  local expected="$2"
  download_verified \
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/${MODEL_REVISION}/${name}" \
    "${MODEL_DIR}/${name}" \
    "${expected}"
}

download_verified "${PYTHON_URL}" "${DOWNLOAD_DIR}/${PYTHON_ARCHIVE}" "${PYTHON_SHA256}"
if [[ ! -x "${RUNTIME_DIR}/bin/python3" ]] || ! "${RUNTIME_DIR}/bin/python3" --version >/dev/null 2>&1; then
  rm -rf "${RUNTIME_DIR}"
  mkdir -p "${RUNTIME_DIR}"
  tar -xzf "${DOWNLOAD_DIR}/${PYTHON_ARCHIVE}" -C "${RUNTIME_DIR}" --strip-components=1
fi

"${RUNTIME_DIR}/bin/python3" -m pip install --disable-pip-version-check --upgrade pip
"${RUNTIME_DIR}/bin/python3" -m pip install --disable-pip-version-check -r "${ROOT_DIR}/requirements-macos.txt"

download_model "ggml-small.bin" "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
download_model "ggml-medium.bin" "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"
download_model "ggml-large-v3-turbo.bin" "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"

WHISPER_SOURCE="${CACHE_DIR}/whisper.cpp"
if [[ ! -d "${WHISPER_SOURCE}/.git" ]]; then
  git clone --depth 1 --branch "${WHISPER_TAG}" https://github.com/ggml-org/whisper.cpp.git "${WHISPER_SOURCE}"
fi
if [[ "$(git -C "${WHISPER_SOURCE}" rev-parse HEAD)" != "${WHISPER_COMMIT}" ]]; then
  echo "Unerwarteter whisper.cpp-Commit." >&2
  exit 1
fi
CMAKE_BIN="${RUNTIME_DIR}/bin/cmake"
"${CMAKE_BIN}" -S "${WHISPER_SOURCE}" -B "${WHISPER_SOURCE}/build" \
  -DCMAKE_BUILD_TYPE=Release -DWHISPER_METAL=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON
"${CMAKE_BIN}" --build "${WHISPER_SOURCE}/build" --config Release --parallel
cp "${WHISPER_SOURCE}/build/bin/whisper-cli" "${BIN_DIR}/whisper-cli"
find "${WHISPER_SOURCE}/build/bin" -maxdepth 1 \( -type f -o -type l \) -name '*.dylib' -exec cp -P {} "${LIB_DIR}/" \;
chmod +x "${BIN_DIR}/whisper-cli"
install_name_tool -delete_rpath "${WHISPER_SOURCE}/build/bin" "${BIN_DIR}/whisper-cli" 2>/dev/null || true
install_name_tool -add_rpath "@executable_path/../lib" "${BIN_DIR}/whisper-cli" 2>/dev/null || true

download_verified "${FFMPEG_URL}" "${DOWNLOAD_DIR}/ffmpeg-darwin-arm64.gz" "${FFMPEG_SHA256}"
gzip -dc "${DOWNLOAD_DIR}/ffmpeg-darwin-arm64.gz" > "${BIN_DIR}/ffmpeg"
chmod +x "${BIN_DIR}/ffmpeg"

download_verified "${DENO_URL}" "${DOWNLOAD_DIR}/${DENO_ARCHIVE}" "${DENO_SHA256}"
unzip -oq "${DOWNLOAD_DIR}/${DENO_ARCHIVE}" -d "${BIN_DIR}"
chmod +x "${BIN_DIR}/deno"

if [[ ! -f "${SPEAKER_MODEL_DIR}/hyperparams.yaml" ]]; then
  rm -rf "${SPEAKER_MODEL_DIR}.new"
  HF_HOME="${CACHE_DIR}/huggingface" "${RUNTIME_DIR}/bin/python3" - "${SPEAKER_MODEL_DIR}.new" <<'PY'
import sys
from speechbrain.inference.speaker import EncoderClassifier

EncoderClassifier.from_hparams(
    source="speechbrain/spkrec-ecapa-voxceleb",
    savedir=sys.argv[1],
    run_opts={"device": "cpu"},
)
PY
  rm -rf "${SPEAKER_MODEL_DIR}"
  mkdir -p "${SPEAKER_MODEL_DIR}"
  rsync -aL "${SPEAKER_MODEL_DIR}.new/" "${SPEAKER_MODEL_DIR}/"
  rm -rf "${SPEAKER_MODEL_DIR}.new"
fi

HF_HUB_OFFLINE=1 PYTHONDONTWRITEBYTECODE=1 "${RUNTIME_DIR}/bin/python3" - "${SPEAKER_MODEL_DIR}" <<'PY'
import sys
import spacy
from silero_vad import load_silero_vad
from speechbrain.inference.speaker import EncoderClassifier

spacy.load("de_core_news_sm")
spacy.load("en_core_web_sm")
load_silero_vad()
EncoderClassifier.from_hparams(source=sys.argv[1], savedir=sys.argv[1], run_opts={"device": "cpu"})
PY
"${BIN_DIR}/whisper-cli" --help >/dev/null
"${BIN_DIR}/ffmpeg" -version >/dev/null
"${BIN_DIR}/deno" --version >/dev/null
"${RUNTIME_DIR}/bin/python3" -m yt_dlp --version >/dev/null

echo "Offline-Ressourcen vorbereitet: ${RESOURCE_DIR}"
