//! Wawona Terminal.
//!
//! The screen is Rust. UniFFI hands styled lines to a host. SwiftUI, Compose,
//! and GTK draw those lines. This crate does not start a Wayland client.

mod screen;

#[cfg(feature = "gtk")]
mod gtk_view;

use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!("wawona_terminal");

#[derive(uniffi::Record)]
pub struct TerminalSpan {
    pub text: String,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(uniffi::Record)]
pub struct TerminalLine {
    pub spans: Vec<TerminalSpan>,
}

#[derive(uniffi::Object)]
pub struct TerminalSession {
    inner: Mutex<screen::Term>,
}

#[uniffi::export]
impl TerminalSession {
    #[uniffi::constructor]
    pub fn new(cols: u32, rows: u32) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(screen::Term::new(cols as usize, rows as usize)),
        })
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        if let Ok(mut term) = self.inner.lock() {
            term.resize(cols as usize, rows as usize);
        }
    }

    pub fn feed(&self, bytes: Vec<u8>) {
        if let Ok(mut term) = self.inner.lock() {
            term.feed(&bytes);
        }
    }

    pub fn take_reply(&self) -> Vec<u8> {
        self.inner
            .lock()
            .map(|mut term| term.drain_reply())
            .unwrap_or_default()
    }

    pub fn lines(&self) -> Vec<TerminalLine> {
        let Ok(term) = self.inner.lock() else {
            return Vec::new();
        };
        let count = term.line_count();
        let mut lines = Vec::with_capacity(count);
        for index in 0..count {
            lines.push(TerminalLine {
                spans: spans_for(&term, index),
            });
        }
        lines
    }
}

fn spans_for(term: &screen::Term, index: usize) -> Vec<TerminalSpan> {
    let cols = term.cols();
    if cols == 0 {
        return Vec::new();
    }
    let mut cells = vec![
        screen::WwnTermCell {
            cp: 0,
            fg: 0,
            bg: 0,
            attrs: 0,
        };
        cols
    ];
    let n = term.line_cells(index, &mut cells);
    let mut end = n;
    while end > 0 {
        let cell = cells[end - 1];
        if (cell.cp == 0 || cell.cp == u32::from(b' ')) && cell.bg == 0 {
            end -= 1;
        } else {
            break;
        }
    }
    let mut spans = Vec::new();
    let mut text = String::new();
    let mut style: Option<(u32, u32, u32)> = None;
    for cell in cells.iter().take(end) {
        let key = (cell.fg, cell.bg, cell.attrs);
        if style != Some(key) {
            push_span(&mut spans, &mut text, style);
            style = Some(key);
        }
        let cp = char::from_u32(cell.cp).unwrap_or(' ');
        text.push(cp);
    }
    push_span(&mut spans, &mut text, style);
    spans
}

fn push_span(spans: &mut Vec<TerminalSpan>, text: &mut String, style: Option<(u32, u32, u32)>) {
    if text.is_empty() {
        return;
    }
    let (fg, bg, attrs) = style.unwrap_or((0xE8E8E8, 0, 0));
    spans.push(TerminalSpan {
        text: std::mem::take(text),
        fg,
        bg,
        bold: attrs & 1 != 0,
        italic: attrs & 4 != 0,
        underline: attrs & 8 != 0,
    });
}
