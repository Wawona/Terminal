#!/usr/bin/env bash
# Build Rust libghostty-rs static archive for one Wawona product ABI.
# Usage: ./scripts/build-static.sh <abi>
# Output: out/<abi>/libghostty.a (+ include/ghostty.h)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_ROOT="${ROOT}/out"
ABI="${1:?abi required}"
export PATH="${HOME}/.cargo/bin:${PATH}"

if [[ ! -f "${ROOT}/Cargo.toml" ]]; then
  echo "missing Terminal Cargo.toml at ${ROOT}" >&2
  exit 1
fi
if ! command -v cargo >/dev/null; then
  echo "cargo required" >&2
  exit 1
fi

TARGET=""
APPLE_MIN=""
case "${ABI}" in
  macos) TARGET="aarch64-apple-darwin"; APPLE_MIN="13.0" ;;
  macos-x86_64) TARGET="x86_64-apple-darwin"; APPLE_MIN="13.0" ;;
  ios) TARGET="aarch64-apple-ios"; APPLE_MIN="13.0" ;;
  ios-sim) TARGET="aarch64-apple-ios-sim"; APPLE_MIN="13.0" ;;
  tvos) TARGET="aarch64-apple-tvos"; APPLE_MIN="13.0" ;;
  tvos-sim) TARGET="aarch64-apple-tvos-sim"; APPLE_MIN="13.0" ;;
  visionos) TARGET="aarch64-apple-visionos"; APPLE_MIN="1.0" ;;
  visionos-sim) TARGET="aarch64-apple-visionos-sim"; APPLE_MIN="1.0" ;;
  watchos) TARGET="aarch64-apple-watchos"; APPLE_MIN="9.0" ;;
  watchos-sim) TARGET="aarch64-apple-watchos-sim"; APPLE_MIN="9.0" ;;
  android) TARGET="aarch64-linux-android" ;;
  linux) TARGET="aarch64-unknown-linux-gnu" ;;
  linux-x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  *) echo "unknown abi: ${ABI}" >&2; exit 1 ;;
esac

OUT="${OUT_ROOT}/${ABI}"
mkdir -p "${OUT}/include"
cp -f "${ROOT}/include/ghostty.h" "${OUT}/include/ghostty.h"

cd "${ROOT}"
rustup target add "${TARGET}" 2>/dev/null || true

export CARGO_TARGET_DIR="${ROOT}/target/${ABI}"
case "${ABI}" in
  ios|ios-sim) export IPHONEOS_DEPLOYMENT_TARGET="${APPLE_MIN}" ;;
  tvos|tvos-sim) export TVOS_DEPLOYMENT_TARGET="${APPLE_MIN}" ;;
  watchos|watchos-sim) export WATCHOS_DEPLOYMENT_TARGET="${APPLE_MIN}" ;;
  macos|macos-x86_64) export MACOSX_DEPLOYMENT_TARGET="${APPLE_MIN}" ;;
esac

# When Nix supplies Ghostty VT (PKG_CONFIG_PATH / nix-ghostty-vt), skip cargo zig.
FEATURES=( )
if [[ -n "${PKG_CONFIG_PATH:-}" ]] || [[ "${WAWONA_TERMINAL_NIX_VT:-0}" == "1" ]]; then
  FEATURES+=( --features nix-ghostty-vt )
fi
echo "+ cargo build -p wawona-terminal --release --target ${TARGET} ${FEATURES[*]:-}"
# Standalone / product hosts need the real libghostty-rs grid. Stub empty
# viewport looks like a "working" app with blank Metal/GTK. Fail closed when
# WAWONA_TERMINAL_FAIL_STUB=1 (nix run .#terminal) or ABI is macos*.
FAIL_STUB=0
if [[ "${WAWONA_TERMINAL_FAIL_STUB:-0}" == "1" ]]; then
  FAIL_STUB=1
fi
case "${ABI}" in
  macos|macos-x86_64) FAIL_STUB=1 ;;
esac
if ! cargo build -p wawona-terminal --release --target "${TARGET}" "${FEATURES[@]}" 2>"${OUT}/build.log"; then
  if [[ "${FAIL_STUB}" -eq 1 ]]; then
    echo "FAIL: cargo target ${TARGET} (real libghostty-rs required)" >&2
    tail -40 "${OUT}/build.log" || true
    exit 1
  fi
  echo "warn: cargo target ${TARGET} failed; building host stub archive" >&2
  tail -20 "${OUT}/build.log" || true
  STUB_C="${ROOT}/bootstrap/ghostty_stub.c"
  mkdir -p "${ROOT}/bootstrap"
  if [[ ! -f "${STUB_C}" ]]; then
    cat >"${STUB_C}" <<'STUB'
#include <stddef.h>
#include <stdint.h>
int ghostty_init(void) { return 0; }
void ghostty_shutdown(void) {}
void *ghostty_surface_new(uint32_t c, uint32_t r) { (void)c;(void)r; return (void*)(uintptr_t)1; }
void ghostty_surface_free(void *s) { (void)s; }
void ghostty_surface_resize(void *s, uint32_t c, uint32_t r) { (void)s;(void)c;(void)r; }
void ghostty_surface_write(void *s, const uint8_t *b, size_t n) { (void)s;(void)b;(void)n; }
void ghostty_surface_key(void *s, uint32_t k, uint32_t m, int d) { (void)s;(void)k;(void)m;(void)d; }
size_t ghostty_surface_take_reply(void *s, uint8_t *b, size_t c) { (void)s;(void)b;(void)c; return 0; }
uint32_t ghostty_surface_viewport(const void *s, uint32_t o, void *out, uint32_t cap, uint32_t *c, uint32_t *r) {
  (void)s;(void)o;(void)out;(void)cap; if(c)*c=0; if(r)*r=0; return 0;
}
uint32_t ghostty_surface_origin(const void *s) { (void)s; return 0; }
uint32_t ghostty_surface_title(const void *s, uint8_t *o, uint32_t c) { (void)s;(void)o;(void)c; return 0; }
void ghostty_surface_set_cell_px(void *s, uint32_t w, uint32_t h) { (void)s;(void)w;(void)h; }
void ghostty_surface_pointer(void *s, uint8_t b, uint32_t c, uint32_t r, int p, int m) {
  (void)s;(void)b;(void)c;(void)r;(void)p;(void)m;
}
char *ghostty_surface_encode_paste(const void *s, const uint8_t *b, size_t n) { (void)s;(void)b;(void)n; return NULL; }
void ghostty_string_free(char *p) { (void)p; }
STUB
  fi
  OBJ="${OUT}/ghostty_stub.o"
  clang -c -O2 -fPIC "${STUB_C}" -o "${OBJ}"
  ar rcs "${OUT}/libghostty.a" "${OBJ}"
else
  LIB="${CARGO_TARGET_DIR}/${TARGET}/release/libwawona_terminal.a"
  if [[ ! -f "${LIB}" ]]; then
    LIB=$(find "${CARGO_TARGET_DIR}" -name 'libwawona_terminal.a' | head -1)
  fi
  cp -f "${LIB}" "${OUT}/libwawona_terminal.a"
  cp -f "${OUT}/libwawona_terminal.a" "${OUT}/libghostty.a"
fi

if ! (nm -g "${OUT}/libghostty.a" 2>/dev/null || true) | grep -q 'ghostty_init'; then
  if ! grep -a -q 'ghostty_init' "${OUT}/libghostty.a"; then
    echo "FAIL: ghostty_init missing in ${OUT}/libghostty.a" >&2
    exit 1
  fi
fi
echo "OK ${OUT}/libwawona_terminal.a (alias libghostty.a)"
ls -la "${OUT}/libwawona_terminal.a" "${OUT}/libghostty.a"
