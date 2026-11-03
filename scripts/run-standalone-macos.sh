#!/usr/bin/env bash
# Build + launch standalone Wawona Terminal (macOS Metal + libghostty-rs).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${WAWONA_TERMINAL_APP:-/tmp/WawonaTerminal.app}"
LIB="$ROOT/out/macos/libghostty.a"
SRC="$ROOT/apple/Standalone/StandaloneTermSurface.swift"
PTY_C="$ROOT/apple/Standalone/pty_spawn.c"
OBJDIR="$(mktemp -d /tmp/wawona-term-build.XXXXXX)"

if [[ ! -f "$LIB" ]]; then
  echo "missing $LIB; run scripts/build-static.sh macos" >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cat >"$OUT/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>WawonaTerminal</string>
  <key>CFBundleIdentifier</key><string>com.aspauldingcode.wawona.terminal</string>
  <key>CFBundleName</key><string>Wawona Terminal</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>26.11.3</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

SDK="$(xcrun --sdk macosx --show-sdk-path)"
clang -c -O2 -isysroot "$SDK" -target arm64-apple-macos13.0 \
  "$PTY_C" -o "$OBJDIR/pty_spawn.o"
swiftc -O -parse-as-library \
  -sdk "$SDK" \
  -target arm64-apple-macos13.0 \
  -framework AppKit -framework Metal -framework MetalKit -framework CoreText \
  -lutil \
  -I "$ROOT/include" \
  "$SRC" \
  "$OBJDIR/pty_spawn.o" \
  "$LIB" \
  -o "$OUT/Contents/MacOS/WawonaTerminal"
rm -rf "$OBJDIR"

codesign --force -s - "$OUT" >/dev/null 2>&1 || true
echo "built $OUT"
if [[ "${WAWONA_TERMINAL_NO_OPEN:-0}" != "1" ]]; then
  open "$OUT"
fi
