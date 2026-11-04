#!/usr/bin/env bash
# Build + launch standalone Wawona Terminal (macOS Metal + libghostty-rs).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${WAWONA_TERMINAL_APP:-/tmp/WawonaTerminal.app}"
# Prefer Nix/cargo product archive; libghostty.a is the build-static.sh alias.
if [[ -f "$ROOT/out/macos/libwawona_terminal.a" ]]; then
  LIB="$ROOT/out/macos/libwawona_terminal.a"
elif [[ -f "$ROOT/out/macos/libghostty.a" ]]; then
  LIB="$ROOT/out/macos/libghostty.a"
else
  echo "missing out/macos/libwawona_terminal.a (or libghostty.a); nix build .#terminal or scripts/build-static.sh macos" >&2
  exit 1
fi
GPU="$ROOT/apple/Terminal/GhosttyGpuView.swift"
FONT_ENV="$ROOT/apple/Terminal/TerminalFontEnvironment.swift"
SRC="$ROOT/apple/Standalone/StandaloneTermSurface.swift"
PTY_C="$ROOT/apple/Standalone/pty_spawn.c"
OBJDIR="$(mktemp -d /tmp/wawona-term-build.XXXXXX)"

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
  "$FONT_ENV" \
  "$GPU" \
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
