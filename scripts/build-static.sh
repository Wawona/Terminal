#!/usr/bin/env bash
# Build Rust libghostty-rs static archive for one Wawona product ABI.
# Usage: ./scripts/build-static.sh <abi>
# Output: out/<abi>/libghostty.a (+ include/wwn_term.h, ghostty.h shim)
#
# Fail closed: never emit an empty stub archive. Real libghostty-rs only.
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
cp -f "${ROOT}/include/wwn_term.h" "${OUT}/include/wwn_term.h"
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
if ! cargo build -p wawona-terminal --release --target "${TARGET}" "${FEATURES[@]}" 2>"${OUT}/build.log"; then
  echo "FAIL: cargo target ${TARGET} (real libghostty-rs required; stub archives removed)" >&2
  tail -40 "${OUT}/build.log" || true
  exit 1
fi

LIB="${CARGO_TARGET_DIR}/${TARGET}/release/libwawona_terminal.a"
if [[ ! -f "${LIB}" ]]; then
  LIB=$(find "${CARGO_TARGET_DIR}" -name 'libwawona_terminal.a' ! -path '*/deps/*' ! -path '*/build/*' | head -1)
fi
if [[ -z "${LIB}" || ! -f "${LIB}" ]]; then
  echo "FAIL: libwawona_terminal.a missing after cargo build" >&2
  exit 1
fi
cp -f "${LIB}" "${OUT}/libwawona_terminal.a"
cp -f "${OUT}/libwawona_terminal.a" "${OUT}/libghostty.a"

if ! (nm -g "${OUT}/libghostty.a" 2>/dev/null || true) | grep -q 'wwn_term_new'; then
  if ! grep -a -q 'wwn_term_new' "${OUT}/libghostty.a"; then
    echo "FAIL: wwn_term_new missing in ${OUT}/libghostty.a" >&2
    exit 1
  fi
fi
# Phase 5: Rust must not export Zig GhosttyLib names.
if (nm -g "${OUT}/libghostty.a" 2>/dev/null || true) | grep -q ' T _ghostty_surface_new$'; then
  echo "FAIL: Rust still exports ghostty_surface_new (ABI collide with Zig)" >&2
  exit 1
fi
if grep -a -q $'\0ghostty_surface_new\0' "${OUT}/libghostty.a" 2>/dev/null; then
  : # mangled strings may appear; prefer nm above
fi
echo "OK ${OUT}/libwawona_terminal.a (alias libghostty.a; wwn_term_* ABI)"
ls -la "${OUT}/libwawona_terminal.a" "${OUT}/libghostty.a"
