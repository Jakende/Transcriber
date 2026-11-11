#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -f .local-build.env ]]; then source .local-build.env; fi
export MACOSX_DEPLOYMENT_TARGET=13.0
export COPYFILE_DISABLE=1
bundle_model=""
if [[ ${1:-} == --bundle-model ]]; then bundle_model="${2:?Model name required: tiny, small, medium, turbo}"; shift 2; fi
if [[ $# -gt 0 ]]; then echo "Usage: $0 [--bundle-model small]" >&2; exit 2; fi
cargo build --release --locked
# cargo metadata respects both CARGO_TARGET_DIR and user Cargo configuration.
target_dir="$(cargo metadata --no-deps --format-version 1 | /usr/bin/plutil -extract target_directory raw -o - -)"
binary="$target_dir/release/transcription-rust"
dist="${TRANSCRIPTION_DIST_DIR:-$PWD/dist}"
app="$dist/Transcription Rust.app"
if [[ -e "$app" ]]; then
  backup="$dist/Transcription Rust.previous.$(date +%Y%m%dT%H%M%S).app"
  mv "$app" "$backup"
fi
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Transcription Rust"
cp LICENSE THIRD-PARTY-NOTICES.md README.md PARITY.md VALIDATION-0.2.md "$app/Contents/Resources/"
gzip -dc THIRD-PARTY-LICENSES.html.gz > "$app/Contents/Resources/THIRD-PARTY-LICENSES.html"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Transcription Rust</string>
<key>CFBundleIdentifier</key><string>de.jakende.transcription-rust</string>
<key>CFBundleName</key><string>Transcription Rust</string>
<key>CFBundleDisplayName</key><string>Transcription Rust</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.2.0</string>
<key>CFBundleVersion</key><string>2</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
<key>NSHumanReadableCopyright</key><string>Transcription Rust contributors</string>
</dict></plist>
PLIST
# Ship only small tokenizer/configuration files by default; existing weights stay in place.
"$binary" prepare-metadata
metadata_root="$("$binary" doctor | sed -n 's/^Modelle: //p')"
for name in tiny small medium large-v3-turbo; do
  mkdir -p "$app/Contents/Resources/model-metadata/$name"
  cp "$metadata_root/$name/config.json" "$metadata_root/$name/tokenizer.json" "$app/Contents/Resources/model-metadata/$name/"
done
if [[ -n "$bundle_model" ]]; then
  "$binary" bundle-model "$bundle_model" "$app/Contents/Resources/models"
fi
# Remove Finder/resource-fork metadata before signing, including on ExFAT volumes.
xattr -cr "$app"
codesign --force --deep --sign - "$app"
"./scripts/verify-macos.sh" "$app"
archive="$dist/Transcription-Rust-macOS-arm64.zip"
if [[ -e "$archive" ]]; then mv "$archive" "$archive.previous.$(date +%Y%m%dT%H%M%S)"; fi
ditto --norsrc -c -k --keepParent "$app" "$archive"
(cd "$dist" && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")
printf 'App: %s\nArchiv: %s\n' "$app" "$archive"
