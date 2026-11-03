# Wawona Terminal / libghostty-rs

Rust UniFFI grid for **Wawona Terminal**. Native UI frameworks paint cells.

Same product split as **ToolbarKeys** and Wawona domain UniFFI:

| Rust (this crate) | Native UI |
|-------------------|-----------|
| `GhosttySurface` / `TerminalSession` | `apple/Terminal/` (SwiftUI / UIKit / AppKit) |
| `ghostty_*` / `wwn_term_*` C (optional) | `android/…/WawonaTerminalText.kt` |
| | GTK / Watch SwiftUI hosts in Wawona L4 |

Packaging / flake name: `github.com/Wawona/Ghostty` (`wwn-ghostty`).

Do not link published GhosttyKit. Apple-mobile static only, min OS 13.0.
