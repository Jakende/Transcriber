#!/bin/bash
set -euo pipefail
app="${1:?Usage: verify-macos.sh /path/to/Transcription Rust.app}"
binary="$app/Contents/MacOS/Transcription Rust"
test -x "$binary"
plutil -lint "$app/Contents/Info.plist"
codesign --verify --deep --strict "$app"
file "$binary" | grep -q 'Mach-O 64-bit executable arm64'
# All dynamically linked libraries must ship with macOS.
external="$(otool -L "$binary" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/Library/)' || true)"
if [[ -n "$external" ]]; then printf 'Unexpected runtime dependency:\n%s\n' "$external" >&2; exit 1; fi
"$binary" doctor
"$binary" --version
if find "$app" -type f \( -name '*.py' -o -name '*.dylib' -o -name whisper-cli -o -name ffmpeg -o -name deno \) | grep -q .; then
  echo 'Unexpected external runtime in bundle.' >&2; exit 1
fi
