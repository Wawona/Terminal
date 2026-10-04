# Wawona Terminal

Rust VT screen for Wawona. UniFFI exposes styled lines. Each host draws those
lines with its own toolkit.

| Host | View |
|---|---|
| iOS, iPadOS, tvOS, visionOS, macOS | SwiftUI text scroll in `apple/Terminal` |
| Android | Compose in `android/` |
| Linux | GTK `TextView`, crate feature `gtk` |

This is not foot and not weston-terminal. Those stay Wayland clients. The
iOS native shell is this terminal.

Wawona compiles `apple/Terminal` from this tree. Do not copy that Swift back
into Wawona. Generated UniFFI bindings are a Nix output. Do not commit them.

The crate does not depend on Wawona or iland. iOS deployment stays 13.0.
watchOS draws the same SwiftUI text. It does not link Metal.

The face is DejaVu Sans Mono patched with Nerd Fonts, shipped by Wawona's
font package. Apple system fonts do not include those icons.

```bash
cargo test
cargo build --features gtk   # Linux text view
```
