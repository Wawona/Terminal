# Wawona Terminal

Native terminal for **macOS, iOS, iPadOS, tvOS, watchOS, visionOS, Android,
Linux**. Same product can run **standalone** or **link into Wawona**.

Shape matches **ToolbarKeys** and Wawona domain UniFFI:

| Layer | Owner |
|-------|--------|
| Grid / VT | [`uzaaft/libghostty-rs`](https://github.com/uzaaft/libghostty-rs) (`libghostty-vt` → Ghostty VT) |
| Product UniFFI | This repo: `GhosttySurface` / `TerminalSession` over that engine |
| Optional C ABI | `ghostty_surface_*` / `wwn_term_*` (same engine) |
| Native paint | `apple/Terminal/`, `android/…`, GTK hosts |
| Toolbar keys | `github.com/Wawona/ToolbarKeys` (separate UniFFI; where supported) |
| Machines chrome / PTY in app | Wawona L4 when bundled |

**Wawona Terminal** uses the real Ghostty VT via the Rust `libghostty-vt`
crate. Not GhosttyKit. Not a homemade VT under a libghostty name.

## Layout

```text
Terminal/                 # this repo (flake input: terminal)
  src/                    # UniFFI + C over libghostty-vt
  include/ghostty.h       # optional C ABI for low-level hosts
  scripts/build-static.sh # out/<abi>/libwawona_terminal.a (hosts link VT)
  apple/Terminal/         # SwiftUI / UIKit / AppKit views
  android/…               # Compose text host
```

Wawona stages `apple/Terminal` into `.nix-deps/terminal/Terminal` the same way
it stages ToolbarKeys `apple/Keyboard`.

## Nix build (preferred)

Flake packages Ghostty VT with Zig, then Rust over pkg-config (no host zig in
cargo). Same split as crate2nix for Cargo: **zig2nix** for Zig toolchain,
**ghostty** flake for `libghostty-vt`, Rust `buildRustPackage` / crane for
`wawona-terminal`.

| Package | What |
|---------|------|
| `.#libghostty-vt` | Ghostty VT (Zig `-Demit-lib-vt`) |
| `.#zig` | Zig 0.17 from [zig2nix](https://github.com/Cloudef/zig2nix) |
| `.#terminal` | Product (Linux GTK binary; macOS Metal launcher + Nix staticlib) |

```bash
nix build .#libghostty-vt --accept-flake-config
nix build .#terminal --accept-flake-config
nix run .#terminal --accept-flake-config
nix develop   # zig + pkg-config VT + rust 1.90
```

Cargo feature `nix-ghostty-vt` enables `libghostty-vt-sys/pkg-config` so the
build links the Nix archive instead of fetching/building Ghostty in `build.rs`.

## Scroll and selection APIs

| API | Source |
|-----|--------|
| `scroll_origin` / `set_scroll_origin` / `scroll_delta` / `scroll_to_bottom` | Ghostty `scrollbar().offset` + `scroll_viewport` |
| `selection_text` | Ghostty `format_selection_alloc` (active selection only) |

`selection_text` is empty until the **host** installs a Ghostty selection
(gestures / `Terminal::set_selection`). This crate does **not** invent a
parallel selection model in Rust. Host UI (Metal/UIKit/Compose) owns gesture
→ Ghostty selection; UniFFI/C only read the result.

## Host / local cargo

If you skip Nix, you need Zig matching the libghostty-rs pin (tip: **0.17.x**).
Prefer that repo's `nix develop`, or set `ZIG` / `GHOSTTY_SOURCE_DIR`.

```bash
cargo build --release --features nix-ghostty-vt
cargo test --features nix-ghostty-vt   # or: nix build .#terminal (doCheck)
./scripts/build-static.sh macos        # fail-closed; no stub archive
```

## Standalone

```bash
nix run .#terminal                 # this flake or Wawona `.#terminal`
./scripts/run-standalone-macos.sh  # AppKit Metal dogfood
```

Linux GTK: `cargo run --features standalone,nix-ghostty-vt --bin wawona-terminal`
(with Nix VT on `PKG_CONFIG_PATH`), or plain `standalone` if cargo builds VT.
