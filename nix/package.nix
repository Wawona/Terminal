# Wawona Terminal product package.
#
# Ghostty VT core: Zig-built via ghostty flake (`libghostty-vt`), toolchain from
# zig2nix in the Terminal flake. Rust links through libghostty-vt-sys
# `pkg-config` (same pattern as uzaaft/libghostty-rs). No host zig in cargo.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  gtk4,
  glib,
  clang,
  libclang,
  openssl,
  libiconv,
  writeShellApplication,
  libghosttyVt,
  zig ? null,
  terminalSrc ? ./..,
}:
let
  pname = "wawona-terminal";
  version = "0.1.0";
  src = terminalSrc;

  # Bump when Cargo.toml libghostty-rs rev changes. Fill via:
  #   nix build .#terminal 2>&1 | rg got:
  gitOutputHashes = {
    "libghostty-vt-0.2.1" = "sha256-oy/S2Ir+ltndFcvmMiCyZ5j07w91lKdNxACUJr7I4EU=";
    "libghostty-vt-sys-0.2.1" = "sha256-oy/S2Ir+ltndFcvmMiCyZ5j07w91lKdNxACUJr7I4EU=";
  };

  cargoLock = {
    lockFile = src + "/Cargo.lock";
    outputHashes = gitOutputHashes;
  };

  # Ghostty moves headers, .pc, and static .a into the `dev` output.
  pkgConfigPath = lib.concatStringsSep ":" (
    [ "${libghosttyVt}/share/pkgconfig" ]
    ++ lib.optionals (libghosttyVt ? dev) [ "${libghosttyVt.dev}/share/pkgconfig" ]
  );
  ghosttyVtInputs =
    [ libghosttyVt ] ++ lib.optionals (libghosttyVt ? dev) [ libghosttyVt.dev ];

  linuxBin = rustPlatform.buildRustPackage {
    inherit pname version src cargoLock;
    buildFeatures = [
      "standalone"
      "nix-ghostty-vt"
    ];
    nativeBuildInputs = [
      pkg-config
      clang
    ];
    buildInputs = ghosttyVtInputs ++ [
      libclang
      openssl
      gtk4
      glib
    ];
    PKG_CONFIG_PATH = pkgConfigPath;
    LIBCLANG_PATH = "${libclang.lib}/lib";
    doCheck = false;
    meta = {
      description = "Wawona Terminal standalone (libghostty-rs + Ghostty VT + GTK)";
      mainProgram = "wawona-terminal";
      platforms = lib.platforms.linux;
    };
  };

  darwinLib = rustPlatform.buildRustPackage {
    inherit pname version src cargoLock;
    buildFeatures = [ "nix-ghostty-vt" ];
    # Staticlib only; Metal host links it.
    buildAndTestSubdir = null;
    cargoBuildFlags = [ "--lib" ];
    nativeBuildInputs = [
      pkg-config
      clang
    ];
    buildInputs = ghosttyVtInputs ++ [
      libclang
      openssl
      libiconv
    ];
    PKG_CONFIG_PATH = pkgConfigPath;
    LIBCLANG_PATH = "${libclang.lib}/lib";
    doCheck = false;
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib $out/include
      # Prefer the crate release staticlib. `find … -exec cp` used to leave a
      # stale copy from target/**/build/**/out/ (missing newer C exports).
      lib=""
      for cand in \
        target/macos/aarch64-apple-darwin/release/libwawona_terminal.a \
        target/aarch64-apple-darwin/release/libwawona_terminal.a \
        target/release/libwawona_terminal.a
      do
        if [[ -f "$cand" ]]; then
          lib="$cand"
          break
        fi
      done
      if [[ -z "$lib" ]]; then
        lib="$(find target -path '*/release/libwawona_terminal.a' \
          ! -path '*/deps/*' ! -path '*/build/*' -print -quit)"
      fi
      if [[ -z "$lib" || ! -f "$lib" ]]; then
        echo "installPhase: libwawona_terminal.a not found under target/" >&2
        exit 1
      fi
      cp -f "$lib" "$out/lib/libwawona_terminal.a"
      if [[ -f ${src}/include/ghostty.h ]]; then
        cp -f ${src}/include/ghostty.h $out/include/
      fi
      runHook postInstall
    '';
    meta = {
      description = "Wawona Terminal Rust staticlib (libghostty-rs + Ghostty VT)";
      platforms = lib.platforms.darwin;
    };
  };

  macosLauncher = writeShellApplication {
    name = "wawona-terminal";
    runtimeInputs = lib.optionals (zig != null) [ zig ];
    text = ''
      set -euo pipefail
      export PATH="/usr/bin:/bin:/usr/sbin:/sbin:''${HOME}/.cargo/bin:''${PATH:-}"
      export PKG_CONFIG_PATH="${pkgConfigPath}''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
      export WAWONA_TERMINAL_NIX_LIB="${darwinLib}"
      SRC="${src}"
      CACHE="''${WAWONA_TERMINAL_CACHE:-''${XDG_CACHE_HOME:-$HOME/.cache}/wawona-terminal}"
      APP="$CACHE/WawonaTerminal.app"
      WORK="$CACHE/src"
      STAMP="$CACHE/src-rev"
      if [[ -d "$SRC/.git" ]]; then
        REV="$(cd "$SRC" && git rev-parse HEAD)"
      else
        REV="$(
          find "$SRC" \( -name '*.rs' -o -name '*.swift' -o -name '*.toml' \
            -o -name '*.sh' -o -name '*.c' -o -name '*.h' \) -type f \
            | sort | xargs cat 2>/dev/null | shasum -a 256 | awk '{print $1}'
        )"
      fi
      NEED=0
      if [[ ! -x "$APP/Contents/MacOS/WawonaTerminal" ]]; then
        NEED=1
      elif [[ "$(cat "$STAMP" 2>/dev/null || true)" != "$REV" ]]; then
        NEED=1
      fi
      if [[ "$NEED" -eq 1 ]]; then
        if ! command -v swiftc >/dev/null; then
          echo "Wawona Terminal: swiftc missing (install Xcode)" >&2
          exit 1
        fi
        if ! command -v cargo >/dev/null; then
          echo "Wawona Terminal: cargo missing (install Rust)" >&2
          exit 1
        fi
        mkdir -p "$CACHE"
        chmod -R u+w "$WORK" 2>/dev/null || true
        rm -rf "$WORK"
        mkdir -p "$WORK"
        if command -v rsync >/dev/null; then
          rsync -a --delete \
            --exclude target --exclude out --exclude .git \
            "$SRC"/ "$WORK"/
        else
          cp -R "$SRC"/. "$WORK"/
          rm -rf "$WORK/target" "$WORK/out" "$WORK/.git"
        fi
        chmod -R u+w "$WORK"
        chmod +x "$WORK/scripts/"*.sh
        mkdir -p "$WORK/out/macos/include"
        if [[ -f "$WAWONA_TERMINAL_NIX_LIB/lib/libwawona_terminal.a" ]]; then
          cp -f "$WAWONA_TERMINAL_NIX_LIB/lib/libwawona_terminal.a" "$WORK/out/macos/"
          # Alias for older scripts that still look for libghostty.a.
          cp -f "$WORK/out/macos/libwawona_terminal.a" "$WORK/out/macos/libghostty.a"
          if [[ -f "$WAWONA_TERMINAL_NIX_LIB/include/ghostty.h" ]]; then
            cp -f "$WAWONA_TERMINAL_NIX_LIB/include/ghostty.h" "$WORK/out/macos/include/"
          else
            cp -f "$WORK/include/ghostty.h" "$WORK/out/macos/include/"
          fi
        else
          export WAWONA_TERMINAL_FAIL_STUB=1
          ( cd "$WORK" && cargo build -p wawona-terminal --release \
              --features nix-ghostty-vt --target aarch64-apple-darwin )
          find "$WORK/target" -name 'libwawona_terminal.a' -exec cp -f {} "$WORK/out/macos/" \;
          cp -f "$WORK/out/macos/libwawona_terminal.a" "$WORK/out/macos/libghostty.a"
          cp -f "$WORK/include/ghostty.h" "$WORK/out/macos/include/"
        fi
        WAWONA_TERMINAL_APP="$APP" WAWONA_TERMINAL_NO_OPEN=1 \
          "$WORK/scripts/run-standalone-macos.sh"
        echo "$REV" >"$STAMP"
      fi
      exec open -na "$APP" --args "$@"
    '';
    meta = {
      description = "Wawona Terminal standalone (libghostty-rs + Metal; VT from Nix)";
      platforms = lib.platforms.darwin;
    };
  };
in
if stdenv.hostPlatform.isLinux then
  linuxBin
else if stdenv.hostPlatform.isDarwin then
  macosLauncher
else
  throw "Wawona Terminal standalone: Linux or macOS only"
