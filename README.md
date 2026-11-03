# Wawona Terminal

Native terminal for **macOS, iOS, iPadOS, tvOS, watchOS, visionOS, Android,
Linux**. Same product can run **standalone** or **link into Wawona**.

Shape matches **ToolbarKeys** and Wawona domain UniFFI:

| Layer | Owner |
|-------|--------|
| Grid / VT (**libghostty-rs**) | This repo: Rust UniFFI (`GhosttySurface` / `TerminalSession`) |
| Optional C ABI | `include/ghostty.h` + `ghostty_*` (not the product contract) |
| Native paint | `apple/Terminal/`, `android/…`, GTK hosts |
| Toolbar keys | `github.com/Wawona/ToolbarKeys` (separate UniFFI; where supported) |
| Machines chrome / PTY in app | Wawona L4 when bundled |

Konsole is based on a terminal library. **Wawona Terminal is based on
libghostty-rs**: Rust grid, platform-native UI (AppKit, UIKit, SwiftUI,
Watch, Compose, GTK). Not GhosttyKit. Not Zig. Not a second Ghostty GitHub
repo.

## Layout

```text
Terminal/                 # this repo (flake input: terminal)
  src/                    # libghostty-rs UniFFI + optional C exports
  include/ghostty.h       # optional C ABI for low-level hosts
  scripts/build-static.sh # out/<abi>/libghostty.a
  apple/Terminal/         # SwiftUI / UIKit / AppKit views
  android/…               # Compose text host
```

Wawona stages `apple/Terminal` into `.nix-deps/terminal/Terminal` the same way
it stages ToolbarKeys `apple/Keyboard`.

## Build static (libghostty-rs)

```bash
./scripts/build-static.sh macos
# → out/macos/libghostty.a
```

Apple-mobile: static only, deployment target **13.0**. No product `.dylib`.

## Credits

- Name / role inspiration: Ghostty MIT (Mitchell Hashimoto et al.)
- iOS embed UX start: Rootshell MIT (Kit Knox)
- VT + UniFFI: this repo
