#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Transcription macOS"
PRODUCT_NAME="TranscriptionMacOSApp"
BUNDLE_DIR="${ROOT_DIR}/dist/${APP_NAME}.app"
EXECUTABLE_PATH="${BUNDLE_DIR}/Contents/MacOS/${PRODUCT_NAME}"
RESOURCES_DIR="${BUNDLE_DIR}/Contents/Resources"
ICONSET_DIR="${ROOT_DIR}/dist/AppIcon.iconset"
ICON_PATH="${RESOURCES_DIR}/AppIcon.icns"
RUN_MODE="run"
CLEAN_BUILD=0
BUNDLE_OFFLINE=0
BUILD_CONFIGURATION="debug"

for arg in "$@"; do
  case "${arg}" in
    --clean)
      CLEAN_BUILD=1
      ;;
    --bundle)
      BUNDLE_OFFLINE=1
      BUILD_CONFIGURATION="release"
      ;;
    --verify)
      RUN_MODE="verify"
      ;;
    --no-run)
      RUN_MODE="no-run"
      ;;
    *)
      echo "Unknown argument: ${arg}" >&2
      exit 2
      ;;
  esac
done

cd "${ROOT_DIR}"

if pgrep -x "${PRODUCT_NAME}" >/dev/null 2>&1; then
  pkill -x "${PRODUCT_NAME}" || true
fi

if [[ -x ".build/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" ]] && LC_ALL=C strings ".build/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" | grep -F "/Transcription macOS App/" | grep -Fv "${ROOT_DIR}" >/dev/null 2>&1; then
  CLEAN_BUILD=1
fi

if [[ "${CLEAN_BUILD}" -eq 1 ]]; then
  swift package clean
fi

if [[ "${BUNDLE_OFFLINE}" -eq 1 ]]; then
  "${ROOT_DIR}/script/prepare_offline_bundle.sh"
fi

swift build -c "${BUILD_CONFIGURATION}"

mkdir -p "${BUNDLE_DIR}/Contents/MacOS" "${RESOURCES_DIR}"
cp ".build/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" "${EXECUTABLE_PATH}"
chmod +x "${EXECUTABLE_PATH}"
cp "Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py" "${RESOURCES_DIR}/transcribe_bulk.py"
rm -rf "${RESOURCES_DIR}/transcription_backend"
rsync -a --exclude '__pycache__' "Sources/TranscriptionMacOSApp/Resources/transcription_backend/" "${RESOURCES_DIR}/transcription_backend/"
if [[ "${BUNDLE_OFFLINE}" -eq 1 ]]; then
  ditto "${ROOT_DIR}/.bundle-cache/resources" "${RESOURCES_DIR}"
fi
cp "${ROOT_DIR}/../NOTICE" "${RESOURCES_DIR}/NOTICE"
cp "${ROOT_DIR}/../THIRD-PARTY-LICENSES.md" "${RESOURCES_DIR}/THIRD-PARTY-LICENSES.md"
python3 "${ROOT_DIR}/script/generate_app_icon.py" "${ICONSET_DIR}"
iconutil -c icns "${ICONSET_DIR}" -o "${ICON_PATH}"

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
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>2.0</string>
  <key>CFBundleVersion</key>
  <string>4</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

if [[ "${BUNDLE_OFFLINE}" -eq 1 ]]; then
  find "${BUNDLE_DIR}/Contents/Resources" -type f -name '*.pyc' -delete
  find "${BUNDLE_DIR}/Contents/Resources" -depth -type d -name '__pycache__' -exec rmdir {} \; 2>/dev/null || true
  codesign --force --deep --sign - "${BUNDLE_DIR}"
fi

if [[ "${RUN_MODE}" == "verify" ]]; then
  /usr/bin/open -n "${BUNDLE_DIR}"
  sleep 2
  pgrep -x "${PRODUCT_NAME}" >/dev/null
  echo "Verified running process: ${PRODUCT_NAME}"
elif [[ "${RUN_MODE}" == "no-run" ]]; then
  echo "Built app bundle: ${BUNDLE_DIR}"
else
  /usr/bin/open -n "${BUNDLE_DIR}"
  echo "Launched app bundle: ${BUNDLE_DIR}"
fi
