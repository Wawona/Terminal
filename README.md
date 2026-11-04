# Wawona Terminal

Native terminal for **macOS, iOS, iPadOS, tvOS, watchOS, visionOS, Android,
Linux**. Same product can run **standalone** or **link into Wawona**.

| Layer | Owner |
|-------|--------|
| Grid / VT | [`uzaaft/libghostty-rs`](https://github.com/uzaaft/libghostty-rs) (`libghostty-vt`) |
| Product UniFFI | This repo: `GhosttySurface` / `TerminalSession` |
| Optional C ABI | `wwn_term_*` |
| Native paint | `apple/Terminal/`, Android Compose, Linux GTK/Pango |
| Shell backend | System login shell (macOS/Linux/Mode B) or Wawona PTY (Mode A) |
| Toolbar keys | `github.com/Wawona/ToolbarKeys` (separate; where supported) |
| Machines chrome | Wawona L4 when bundled |

**Not used as product path:** GhosttyKit, Zig `ghostty_surface_*` host,
homemade VT, growing the interim `GhosttyGpuView` atlas.

## Layout

```text
Terminal/
  src/                    # UniFFI + C over libghostty-vt
  include/wwn_term.h      # VT C ABI
  apple/Terminal/         # SwiftUI / UIKit / AppKit (+ TerminalFontEnvironment)
  apple/Standalone/       # macOS Metal standalone host
  android/…               # Compose host
  scripts/                # static build + standalone launcher
```

Wawona stages `apple/Terminal` into `.nix-deps/terminal/Terminal` the same
way it stages ToolbarKeys `apple/Keyboard`.

## Nix build (preferred)

```bash
nix build .#libghostty-vt --accept-flake-config
nix build .#terminal --accept-flake-config
nix run .#terminal --accept-flake-config
```

From Wawona with a sibling checkout:

```bash
nix run .#terminal --override-input terminal path:../Terminal
```

Standalone macOS must not depend on WawonaApple types. Font registration is
`TerminalFontEnvironment` in this repo.

## Shell backends

See Wawona rule `wawona-terminal-shell-backend`. Rust owns spawn/attach.
Hosts never hardcode `/bin/zsh` vs in-process zsh.

## Credits

Ghostty MIT. libghostty-rs. Rootshell MIT for iOS embed UX ideas only.
