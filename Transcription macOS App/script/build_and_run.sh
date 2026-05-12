#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Transcription macOS"
PRODUCT_NAME="TranscriptionMacOSApp"
BUNDLE_DIR="${ROOT_DIR}/dist/${APP_NAME}.app"
EXECUTABLE_PATH="${BUNDLE_DIR}/Contents/MacOS/${PRODUCT_NAME}"
RESOURCES_DIR="${BUNDLE_DIR}/Contents/Resources"

cd "${ROOT_DIR}"

if pgrep -x "${PRODUCT_NAME}" >/dev/null 2>&1; then
  pkill -x "${PRODUCT_NAME}" || true
fi

swift build

mkdir -p "${BUNDLE_DIR}/Contents/MacOS" "${RESOURCES_DIR}"
cp ".build/debug/${PRODUCT_NAME}" "${EXECUTABLE_PATH}"
chmod +x "${EXECUTABLE_PATH}"
cp "Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py" "${RESOURCES_DIR}/transcribe_bulk.py"

cat > "${BUNDLE_DIR}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>${PRODUCT_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>de.jakende.transcription-macos</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

if [[ "${1:-}" == "--verify" ]]; then
  /usr/bin/open -n "${BUNDLE_DIR}"
  sleep 2
  pgrep -x "${PRODUCT_NAME}" >/dev/null
  echo "Verified running process: ${PRODUCT_NAME}"
elif [[ "${1:-}" == "--no-run" ]]; then
  echo "Built app bundle: ${BUNDLE_DIR}"
else
  /usr/bin/open -n "${BUNDLE_DIR}"
  echo "Launched app bundle: ${BUNDLE_DIR}"
fi
