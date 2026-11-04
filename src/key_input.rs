//! Map host key events into libghostty-vt key encoding.
//!
//! Host `keycode` matches [`libghostty_vt::key::Key`] discriminants.
//! `mods` matches Ghostty `MODS_*` bits (SHIFT=1, CTRL=2, ALT=4, SUPER=8).
//! `action`: 0=press, 1=release, 2=repeat.
//!
//! TODO(S3): full kitty / modifyOtherKeys / app-cursor coverage once hosts
//! feed complete KeyEvent properties (unshifted codepoint, composing, sides).

use libghostty_vt::key::{Action, Encoder, Event, Key, Mods};
use libghostty_vt::terminal::Terminal;

/// Encode one key into terminal bytes using Ghostty's key encoder.
pub fn encode_key(
    terminal: &Terminal<'_, '_>,
    keycode: u32,
    mods: u32,
    action: u32,
    text: &[u8],
) -> Vec<u8> {
    let Ok(mut enc) = Encoder::new() else {
        return fallback_encode(keycode, text);
    };
    let _ = enc.set_options_from_terminal(terminal);

    let Ok(mut event) = Event::new() else {
        return fallback_encode(keycode, text);
    };

    let action = match action {
        1 => Action::Release,
        2 => Action::Repeat,
        _ => Action::Press,
    };
    event.set_action(action);

    let key = Key::try_from(keycode as i32).unwrap_or(Key::Unidentified);
    event.set_key(key);
    event.set_mods(Mods::from_bits_truncate(mods as u16));

    if !text.is_empty() {
        if let Ok(s) = std::str::from_utf8(text) {
            // Ghostty wants unmodified layout text, not C0 controls.
            if !s.chars().any(|c| c.is_control()) {
                event.set_utf8(Some(s.to_string()));
            }
        }
    }

    let mut out = Vec::with_capacity(32);
    if enc.encode_to_vec(&event, &mut out).is_ok() && !out.is_empty() {
        return out;
    }
    fallback_encode(keycode, text)
}

/// Minimal legacy sequences when Ghostty encoder returns empty / errors.
fn fallback_encode(keycode: u32, text: &[u8]) -> Vec<u8> {
    if !text.is_empty() {
        return text.to_vec();
    }
    match Key::try_from(keycode as i32).unwrap_or(Key::Unidentified) {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => b"\x7f".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => b"\x1b[A".to_vec(),
        Key::ArrowDown => b"\x1b[B".to_vec(),
        Key::ArrowRight => b"\x1b[C".to_vec(),
        Key::ArrowLeft => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Space => b" ".to_vec(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::Terminal;

    #[test]
    fn enter_encodes_cr() {
        let term = Terminal::new(80, 24).expect("term");
        let out = encode_key(&term, Key::Enter as i32 as u32, 0, 0, b"");
        assert_eq!(out, b"\r");
    }

    #[test]
    fn printable_text_passes() {
        let term = Terminal::new(80, 24).expect("term");
        let out = encode_key(&term, Key::A as i32 as u32, 0, 0, b"a");
        assert_eq!(out, b"a");
    }

    #[test]
    fn arrow_up_is_csi() {
        let term = Terminal::new(80, 24).expect("term");
        let out = encode_key(&term, Key::ArrowUp as i32 as u32, 0, 0, b"");
        assert!(
            out.starts_with(b"\x1b"),
            "arrow should be an escape sequence, got {out:?}"
        );
    }
}
