#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GLOBAL_PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

exec env PATH="${GLOBAL_PATH}" "${SCRIPT_DIR}/run_transcript_en.sh"
