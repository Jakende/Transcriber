#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GLOBAL_PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

CANDIDATES=(
  "${SCRIPT_DIR}/.venv/bin/python3"
  "/usr/local/bin/python3"
  "/opt/homebrew/bin/python3"
  "$(env PATH="${GLOBAL_PATH}" command -v python3 || true)"
)

PYTHON_BIN=""
for py in "${CANDIDATES[@]}"; do
  [[ -n "${py}" ]] || continue
  [[ -x "${py}" ]] || continue
  if "${py}" -c "import whisper, torch" >/dev/null 2>&1; then
    PYTHON_BIN="${py}"
    break
  fi
done

if [[ -z "${PYTHON_BIN}" ]]; then
  echo "Error: No python3 installation with whisper+torch found."
  echo "Try creating a local environment in ${SCRIPT_DIR}:"
  echo "  python3 -m venv .venv && source .venv/bin/activate && pip install torch openai-whisper"
  for py in "/usr/local/bin/python3" "/opt/homebrew/bin/python3"; do
    if [[ -x "${py}" ]]; then
      echo "Try installing in ${py}: ${py} -m pip install torch openai-whisper"
    fi
  done
  exit 1
fi

echo "Running English transcription script with Python: ${PYTHON_BIN}"
exec "${PYTHON_BIN}" "${SCRIPT_DIR}/transkript_any_audio_en.py"
