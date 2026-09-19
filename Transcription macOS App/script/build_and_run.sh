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
COPY_OFFLINE_RESOURCES=0
BUILD_CONFIGURATION="debug"
SWIFT_SCRATCH_PATH="${TMPDIR:-/tmp}/de.jakende.transcription-macos-swift-build"

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

if [[ -x "${SWIFT_SCRATCH_PATH}/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" ]] && LC_ALL=C strings "${SWIFT_SCRATCH_PATH}/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" | grep -F "/Transcription macOS App/" | grep -Fv "${ROOT_DIR}" >/dev/null 2>&1; then
  CLEAN_BUILD=1
fi

if [[ "${CLEAN_BUILD}" -eq 1 ]]; then
  swift package --scratch-path "${SWIFT_SCRATCH_PATH}" clean
fi

if [[ "${BUNDLE_OFFLINE}" -eq 1 ]]; then
  "${ROOT_DIR}/script/prepare_offline_bundle.sh"
  COPY_OFFLINE_RESOURCES=1
elif [[ -x "${ROOT_DIR}/.bundle-cache/resources/bin/whisper-cli" \
     && -x "${ROOT_DIR}/.bundle-cache/resources/bin/ffmpeg" \
     && -x "${ROOT_DIR}/.bundle-cache/resources/bin/deno" \
     && -x "${ROOT_DIR}/.bundle-cache/resources/python-runtime/bin/python3" \
     && -d "${ROOT_DIR}/.bundle-cache/resources/python-runtime/lib/python3.13/site-packages/yt_dlp" \
     && -f "${ROOT_DIR}/.bundle-cache/resources/models/ggml-small.bin" \
     && -f "${ROOT_DIR}/.bundle-cache/resources/models/ggml-medium.bin" \
     && -f "${ROOT_DIR}/.bundle-cache/resources/models/ggml-large-v3-turbo.bin" ]]; then
  COPY_OFFLINE_RESOURCES=1
  echo "Verwende vorhandene Offline-Ressourcen aus .bundle-cache."
fi

swift build --scratch-path "${SWIFT_SCRATCH_PATH}" -c "${BUILD_CONFIGURATION}"

rm -rf "${BUNDLE_DIR}"
mkdir -p "${BUNDLE_DIR}/Contents/MacOS" "${RESOURCES_DIR}"
cp "${SWIFT_SCRATCH_PATH}/${BUILD_CONFIGURATION}/${PRODUCT_NAME}" "${EXECUTABLE_PATH}"
chmod +x "${EXECUTABLE_PATH}"

# SwiftPM links executable package products with the deployment target as the
# recorded SDK version. On newer macOS releases that opts the app into the
# legacy AppKit/SwiftUI appearance even though it was compiled with the current
# SDK. Record the SDK that actually compiled the binary so native controls use
# the same current-system styling as an ordinary Xcode app build.
SDK_VERSION="$(xcrun --sdk macosx --show-sdk-version)"
SDK_STAMPED_EXECUTABLE="${EXECUTABLE_PATH}.sdk-stamped"
codesign --remove-signature "${EXECUTABLE_PATH}" 2>/dev/null || true
xcrun vtool \
  -set-build-version macos 13.0 "${SDK_VERSION}" \
  -replace \
  -output "${SDK_STAMPED_EXECUTABLE}" \
  "${EXECUTABLE_PATH}"
mv "${SDK_STAMPED_EXECUTABLE}" "${EXECUTABLE_PATH}"
chmod +x "${EXECUTABLE_PATH}"
codesign --force --sign - "${EXECUTABLE_PATH}"

cp "Sources/TranscriptionMacOSApp/Resources/transcribe_bulk.py" "${RESOURCES_DIR}/transcribe_bulk.py"
rm -rf "${RESOURCES_DIR}/transcription_backend"
rsync -a --exclude '__pycache__' "Sources/TranscriptionMacOSApp/Resources/transcription_backend/" "${RESOURCES_DIR}/transcription_backend/"
if [[ "${COPY_OFFLINE_RESOURCES}" -eq 1 ]]; then
  # Python-Bytecode ist nur ein Laufzeit-Cache. Ihn nicht in das App-Bundle zu
  # kopieren spart mehrere Gigabyte und verhindert nachträgliche Signaturfehler.
  rsync -a --exclude '__pycache__/' --exclude '*.pyc' \
    "${ROOT_DIR}/.bundle-cache/resources/" "${RESOURCES_DIR}/"
  # OneDrive materialisiert symbolische Links teilweise als kleine Textdateien,
  # deren Inhalt nur der Zielname ist. Im erzeugten Bundle werden diese Links
  # sicher rekonstruiert, ohne den Cache zu verändern.
  while IFS= read -r candidate; do
    [[ "$(wc -c < "${candidate}")" -le 256 ]] || continue
    link_target="$(LC_ALL=C tr -d '\r\n' < "${candidate}")"
    [[ -n "${link_target}" && "${link_target}" != */* ]] || continue
    if [[ -e "$(dirname "${candidate}")/${link_target}" ]]; then
      rm -f "${candidate}"
      ln -s "${link_target}" "${candidate}"
    fi
  done < <(find "${RESOURCES_DIR}" -type f -size -256c)
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
  <string>2.1</string>
  <key>CFBundleVersion</key>
  <string>5</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

if [[ "${COPY_OFFLINE_RESOURCES}" -eq 1 ]]; then
  find "${BUNDLE_DIR}/Contents/Resources" -type f -name '*.pyc' -delete
  find "${BUNDLE_DIR}/Contents/Resources" -depth -type d -name '__pycache__' -exec rmdir {} \; 2>/dev/null || true
  codesign --force --deep --sign - "${BUNDLE_DIR}"
  "${ROOT_DIR}/script/verify_app_bundle.sh" "${BUNDLE_DIR}"
else
  echo "WARNUNG: Entwicklungsbundle ohne Offline-Laufzeit erstellt." >&2
  echo "Für eine transkriptionsfähige App: ./script/build_and_run.sh --bundle --no-run" >&2
fi

if [[ "${RUN_MODE}" == "verify" ]]; then
  if [[ "${COPY_OFFLINE_RESOURCES}" -ne 1 ]]; then
    echo "Ein App-Starttest ist ohne Offline-Laufzeit nicht möglich." >&2
    exit 1
  fi
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
