//! Wawona Terminal.
//!
//! The screen is Rust. Hosts upload `wwn_term_viewport` and draw it on the GPU.
//! This crate does not link a GPU library.

mod screen;

use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!("wawona_terminal");

/// Live RAM/CPU scrollback recommendation (same probe as `Term::new`).
#[uniffi::export]
pub fn terminal_recommended_scrollback_cap(cols: u32) -> u32 {
    u32::try_from(screen::recommended_scroll_cap(cols as usize)).unwrap_or(u32::MAX)
}

#[derive(uniffi::Record)]
pub struct TerminalSpan {
    pub text: String,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub strike: bool,
}

#[derive(uniffi::Record)]
pub struct TerminalLine {
    pub spans: Vec<TerminalSpan>,
}

#[derive(uniffi::Record)]
pub struct TerminalImage {
    pub id: u32,
    pub column: i32,
    pub row: i32,
    pub x_px: i32,
    pub y_px: i32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub columns: u32,
    pub rows: u32,
    pub z: i32,
    pub rgba: Vec<u8>,
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
            // Columns change bytes-per-row; re-probe available RAM/CPU.
            term.refresh_scroll_cap(1.0);
        }
    }

    pub fn set_scrollback_cap(&self, rows: u32) {
        if let Ok(mut term) = self.inner.lock() {
            term.set_scroll_cap(rows as usize);
        }
    }

    pub fn scrollback_cap(&self) -> u32 {
        self.inner
            .lock()
            .map(|term| u32::try_from(term.scroll_cap()).unwrap_or(u32::MAX))
            .unwrap_or(0)
    }

    /// Re-probe RAM/CPU. `host_factor` in 0.25…1.0 for Low Power / thermal.
    pub fn refresh_scrollback_cap(&self, host_factor: f32) {
        if let Ok(mut term) = self.inner.lock() {
            term.refresh_scroll_cap(host_factor);
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
        term.styled_lines().into_iter().map(uniffi_line).collect()
    }

    pub fn lines_json(&self) -> String {
        self.inner
            .lock()
            .map(|term| term.lines_json())
            .unwrap_or_else(|_| "[]".to_string())
    }

    pub fn encode_paste(&self, text: String) -> Vec<u8> {
        self.inner
            .lock()
            .map(|term| term.encode_paste(&text))
            .unwrap_or_else(|_| text.into_bytes())
    }

    pub fn pointer(&self, button: u8, col: u32, row: u32, pressed: bool, motion: bool) {
        if let Ok(mut term) = self.inner.lock() {
            term.pointer(button, col, row, pressed, motion);
        }
    }

    pub fn title(&self) -> String {
        self.inner
            .lock()
            .map(|term| term.title().to_string())
            .unwrap_or_default()
    }

    pub fn set_cell_px(&self, width: u32, height: u32) {
        if let Ok(mut term) = self.inner.lock() {
            term.set_cell_px(width, height);
        }
    }

    pub fn images(&self) -> Vec<TerminalImage> {
        let Ok(term) = self.inner.lock() else {
            return Vec::new();
        };
        term.placed_images()
            .into_iter()
            .map(|image| TerminalImage {
                id: image.id,
                column: image.col,
                row: image.row,
                x_px: image.x_px,
                y_px: image.y_px,
                pixel_width: image.px_w,
                pixel_height: image.px_h,
                columns: image.cols,
                rows: image.rows,
                z: image.z,
                rgba: image.rgba,
            })
            .collect()
    }
}

fn uniffi_line(line: screen::StyledLine) -> TerminalLine {
    TerminalLine {
        spans: line
            .spans
            .into_iter()
            .map(|span| TerminalSpan {
                text: span.text,
                fg: span.fg,
                bg: span.bg,
                bold: span.bold,
                italic: span.italic,
                underline: span.underline,
                blink: span.blink,
                strike: span.strike,
            })
            .collect(),
    }
}
