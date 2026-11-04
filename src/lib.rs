//! Wawona Terminal grid.
//!
//! VT engine is [libghostty-vt](https://github.com/uzaaft/libghostty-rs) (safe
//! Rust over Ghostty `libghostty-vt`). UniFFI (`GhosttySurface` /
//! `TerminalSession`) and C `wwn_term_*` share that engine. Hosts paint
//! cells (AppKit Metal, UIKit, Compose, GTK). This crate does not link a GPU.
//! Zig GhosttyLib keeps `ghostty_surface_*` (Phase 5 rename).

mod cell;
mod engine;
mod ffi_surface;
mod key_input;

use std::sync::Arc;

uniffi::setup_scaffolding!("wawona_terminal");

/// Live RAM/CPU scrollback recommendation.
#[uniffi::export]
pub fn terminal_recommended_scrollback_cap(cols: u32) -> u32 {
    u32::try_from(engine::recommended_scroll_cap(cols)).unwrap_or(u32::MAX)
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

/// One glyph cell for GPU Present (matches C `WwnTermCell` layout).
#[derive(uniffi::Record, Clone)]
pub struct TerminalCell {
    pub cp: u32,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u32,
}

/// Native Present frame. Hosts paint this with Metal / SpriteKit / GLES / GL.
#[derive(uniffi::Record)]
pub struct TerminalFrame {
    pub cols: u32,
    pub rows: u32,
    pub cells: Vec<TerminalCell>,
    pub cursor_col: u32,
    pub cursor_row: u32,
    pub cursor_visible: bool,
    pub scroll_origin: u32,
    pub scroll_total: u32,
    pub selection_text: String,
}

#[derive(uniffi::Object)]
pub struct TerminalSession {
    inner: engine::VtEngine,
}

#[uniffi::export]
impl TerminalSession {
    #[uniffi::constructor]
    pub fn new(cols: u32, rows: u32) -> Arc<Self> {
        Arc::new(Self {
            inner: engine::VtEngine::new(cols, rows),
        })
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        self.inner.resize(cols, rows);
    }

    pub fn set_scrollback_cap(&self, rows: u32) {
        self.inner.set_scroll_cap(rows as usize);
    }

    pub fn scrollback_cap(&self) -> u32 {
        u32::try_from(self.inner.scroll_cap()).unwrap_or(u32::MAX)
    }

    /// Re-probe RAM/CPU. `host_factor` reserved for Low Power / thermal.
    pub fn refresh_scrollback_cap(&self, _host_factor: f32) {
        let cols = self.inner.cols();
        let cap = engine::recommended_scroll_cap(cols);
        self.inner.set_scroll_cap(cap);
    }

    pub fn feed(&self, bytes: Vec<u8>) {
        self.inner.feed(&bytes);
    }

    pub fn take_reply(&self) -> Vec<u8> {
        self.inner.take_reply()
    }

    pub fn lines(&self) -> Vec<TerminalLine> {
        self.inner
            .styled_lines()
            .into_iter()
            .map(uniffi_line)
            .collect()
    }

    pub fn lines_json(&self) -> String {
        self.inner.lines_json()
    }

    pub fn encode_paste(&self, text: String) -> Vec<u8> {
        self.inner.encode_paste(&text)
    }

    pub fn pointer(&self, button: u8, col: u32, row: u32, pressed: bool, motion: bool) {
        self.inner.pointer(button, col, row, pressed, motion);
    }

    pub fn title(&self) -> String {
        self.inner.title()
    }

    pub fn set_cell_px(&self, width: u32, height: u32) {
        self.inner.set_cell_px(width, height);
    }

    /// Absolute scrollback row (Ghostty scrollbar offset). 0 = top of history.
    pub fn scroll_origin(&self) -> u32 {
        self.inner.origin()
    }

    pub fn set_scroll_origin(&self, origin: u32) {
        self.inner.scroll_to_origin(origin);
    }

    /// Relative scroll. Negative into history, positive toward bottom.
    pub fn scroll_delta(&self, delta: i32) {
        self.inner.scroll_delta(delta);
    }

    pub fn scroll_to_bottom(&self) {
        self.inner.scroll_to_bottom();
    }

    pub fn scroll_total(&self) -> u32 {
        self.inner.scroll_total()
    }

    /// Plain text of Ghostty's active selection, or empty if none.
    pub fn selection_text(&self) -> String {
        self.inner.selection_text()
    }

    /// Encode a host key (`keycode` = libghostty Key discriminant).
    pub fn key(&self, keycode: u32, mods: u32, action: u32, text: Vec<u8>) {
        self.inner.key(keycode, mods, action, &text);
    }

    /// Begin selection. `mode`: 0=cell, 1=word, 2=row.
    pub fn select_begin(&self, col: u32, row: u32, mode: u32) {
        self.inner
            .select_begin(col, row, engine::SelectMode::from_u32(mode));
    }

    pub fn select_extend(&self, col: u32, row: u32) {
        self.inner.select_extend(col, row);
    }

    pub fn select_clear(&self) {
        self.inner.select_clear();
    }

    pub fn select_all(&self) {
        self.inner.select_all();
    }

    pub fn images(&self) -> Vec<TerminalImage> {
        self.inner
            .placed_images()
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

    /// Cell grid for native GPU Present (Metal / SpriteKit / Android / Linux).
    pub fn frame(&self) -> TerminalFrame {
        let f = self.inner.present_frame();
        TerminalFrame {
            cols: f.cols,
            rows: f.rows,
            cells: f
                .cells
                .into_iter()
                .map(|c| TerminalCell {
                    cp: c.cp,
                    fg: c.fg,
                    bg: c.bg,
                    attrs: c.attrs,
                })
                .collect(),
            cursor_col: f.cursor_col,
            cursor_row: f.cursor_row,
            cursor_visible: f.cursor_visible,
            scroll_origin: f.scroll_origin,
            scroll_total: f.scroll_total,
            selection_text: f.selection_text,
        }
    }
}

fn uniffi_line(line: engine::StyledLine) -> TerminalLine {
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

/// Product UniFFI surface. Same VT as [`TerminalSession`].
#[derive(uniffi::Object)]
pub struct GhosttySurface {
    session: Arc<TerminalSession>,
}

#[uniffi::export]
impl GhosttySurface {
    #[uniffi::constructor]
    pub fn new(cols: u32, rows: u32) -> Arc<Self> {
        Arc::new(Self {
            session: TerminalSession::new(cols, rows),
        })
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        self.session.resize(cols, rows);
    }

    pub fn feed(&self, bytes: Vec<u8>) {
        self.session.feed(bytes);
    }

    pub fn take_reply(&self) -> Vec<u8> {
        self.session.take_reply()
    }

    pub fn lines(&self) -> Vec<TerminalLine> {
        self.session.lines()
    }

    pub fn lines_json(&self) -> String {
        self.session.lines_json()
    }

    pub fn encode_paste(&self, text: String) -> Vec<u8> {
        self.session.encode_paste(text)
    }

    pub fn pointer(&self, button: u8, col: u32, row: u32, pressed: bool, motion: bool) {
        self.session.pointer(button, col, row, pressed, motion);
    }

    pub fn title(&self) -> String {
        self.session.title()
    }

    pub fn set_cell_px(&self, width: u32, height: u32) {
        self.session.set_cell_px(width, height);
    }

    pub fn images(&self) -> Vec<TerminalImage> {
        self.session.images()
    }

    pub fn refresh_scrollback_cap(&self, host_factor: f32) {
        self.session.refresh_scrollback_cap(host_factor);
    }

    pub fn scrollback_cap(&self) -> u32 {
        self.session.scrollback_cap()
    }

    pub fn scroll_origin(&self) -> u32 {
        self.session.scroll_origin()
    }

    pub fn set_scroll_origin(&self, origin: u32) {
        self.session.set_scroll_origin(origin);
    }

    pub fn scroll_delta(&self, delta: i32) {
        self.session.scroll_delta(delta);
    }

    pub fn scroll_to_bottom(&self) {
        self.session.scroll_to_bottom();
    }

    pub fn scroll_total(&self) -> u32 {
        self.session.scroll_total()
    }

    pub fn selection_text(&self) -> String {
        self.session.selection_text()
    }

    pub fn key(&self, keycode: u32, mods: u32, action: u32, text: Vec<u8>) {
        self.session.key(keycode, mods, action, text);
    }

    pub fn select_begin(&self, col: u32, row: u32, mode: u32) {
        self.session.select_begin(col, row, mode);
    }

    pub fn select_extend(&self, col: u32, row: u32) {
        self.session.select_extend(col, row);
    }

    pub fn select_clear(&self) {
        self.session.select_clear();
    }

    pub fn select_all(&self) {
        self.session.select_all();
    }

    pub fn frame(&self) -> TerminalFrame {
        self.session.frame()
    }
}

// Re-export cell layout for hosts that compile against this crate.
pub use cell::WwnTermCell;
