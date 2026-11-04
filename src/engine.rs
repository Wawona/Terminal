//! Product VT: [libghostty-vt](https://github.com/uzaaft/libghostty-rs) (Rust API
//! over Ghostty `libghostty-vt`). Homemade VT is gone.
//!
//! `Terminal` is `!Send` / `!Sync`. Own it on one OS thread; UniFFI / C hosts
//! talk through this Send-safe handle.

// `super::` so this file also compiles as Wawona `src/term/engine.rs`.
use super::cell::{
    ATTR_BLINK, ATTR_BOLD, ATTR_CURSOR, ATTR_DIM, ATTR_ITALIC, ATTR_STRIKE, ATTR_UNDERLINE,
    WwnTermCell, pack_rgb,
};
use libghostty_vt::render::{CellIterator, Dirty, RowIterator};
use libghostty_vt::style::{RgbColor, Style, StyleColor, Underline};
use libghostty_vt::terminal::ScrollViewport;
use libghostty_vt::{RenderState, Terminal, paste};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

const DEFAULT_SCROLLBACK: usize = 10_000;

#[derive(Clone, Debug, Default)]
pub struct StyledSpan {
    pub text: String,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub strike: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

#[derive(Clone, Debug, Default)]
pub struct PlacedImage {
    pub id: u32,
    pub col: i32,
    pub row: i32,
    pub x_px: i32,
    pub y_px: i32,
    pub px_w: u32,
    pub px_h: u32,
    pub cols: u32,
    pub rows: u32,
    pub z: i32,
    pub rgba: Vec<u8>,
}

struct Snapshot {
    cols: u16,
    rows: u16,
    cells: Vec<WwnTermCell>,
    lines: Vec<StyledLine>,
    title: String,
    reply: Vec<u8>,
    scroll_cap: usize,
    cell_w: u32,
    cell_h: u32,
    gen: u64,
    cursor_col: u16,
    cursor_row: u16,
    cursor_visible: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            cells: Vec::new(),
            lines: Vec::new(),
            title: String::new(),
            reply: Vec::new(),
            scroll_cap: DEFAULT_SCROLLBACK,
            cell_w: 8,
            cell_h: 16,
            gen: 0,
            cursor_col: 0,
            cursor_row: 0,
            cursor_visible: true,
        }
    }
}

enum Cmd {
    Feed(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    SetCellPx { w: u32, h: u32 },
    SetScrollCap(usize),
    ScrollToOrigin(u32),
    Pointer {
        button: u8,
        col: u32,
        row: u32,
        pressed: bool,
        motion: bool,
    },
    Refresh { done: Sender<()> },
    Shutdown,
}

/// Send-safe handle to a Ghostty VT worker.
pub struct VtEngine {
    tx: Sender<Cmd>,
    snap: Arc<Mutex<Snapshot>>,
    join: Option<JoinHandle<()>>,
}

impl VtEngine {
    pub fn new(cols: u32, rows: u32) -> Self {
        let cols = cols.clamp(1, 512) as u16;
        let rows = rows.clamp(1, 256) as u16;
        let (tx, rx) = mpsc::channel::<Cmd>();
        let snap = Arc::new(Mutex::new(Snapshot {
            cols,
            rows,
            ..Snapshot::default()
        }));
        let snap_worker = Arc::clone(&snap);
        let join = thread::Builder::new()
            .name("wawona-ghostty-vt".into())
            .spawn(move || worker(rx, snap_worker, cols, rows))
            .expect("spawn ghostty-vt worker");
        let engine = Self {
            tx,
            snap,
            join: Some(join),
        };
        engine.sync_refresh();
        engine
    }

    fn sync_refresh(&self) {
        let (done_tx, done_rx) = mpsc::channel();
        if self.tx.send(Cmd::Refresh { done: done_tx }).is_ok() {
            let _ = done_rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    }

    pub fn feed(&self, bytes: &[u8]) {
        let _ = self.tx.send(Cmd::Feed(bytes.to_vec()));
        self.sync_refresh();
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        let cols = cols.clamp(1, 512) as u16;
        let rows = rows.clamp(1, 256) as u16;
        let _ = self.tx.send(Cmd::Resize { cols, rows });
        self.sync_refresh();
    }

    pub fn set_cell_px(&self, width: u32, height: u32) {
        let _ = self.tx.send(Cmd::SetCellPx {
            w: width.max(1),
            h: height.max(1),
        });
        self.sync_refresh();
    }

    pub fn set_scroll_cap(&self, rows: usize) {
        let _ = self.tx.send(Cmd::SetScrollCap(rows.max(1)));
        self.sync_refresh();
    }

    pub fn scroll_cap(&self) -> usize {
        self.snap
            .lock()
            .map(|s| s.scroll_cap)
            .unwrap_or(DEFAULT_SCROLLBACK)
    }

    pub fn scroll_to_origin(&self, origin: u32) {
        let _ = self.tx.send(Cmd::ScrollToOrigin(origin));
        self.sync_refresh();
    }

    pub fn pointer(&self, button: u8, col: u32, row: u32, pressed: bool, motion: bool) {
        let _ = self.tx.send(Cmd::Pointer {
            button,
            col,
            row,
            pressed,
            motion,
        });
        self.sync_refresh();
    }

    pub fn take_reply(&self) -> Vec<u8> {
        self.snap
            .lock()
            .map(|mut s| std::mem::take(&mut s.reply))
            .unwrap_or_default()
    }

    pub fn title(&self) -> String {
        self.snap
            .lock()
            .map(|s| s.title.clone())
            .unwrap_or_default()
    }

    pub fn cols(&self) -> u32 {
        self.snap.lock().map(|s| u32::from(s.cols)).unwrap_or(80)
    }

    pub fn rows(&self) -> u32 {
        self.snap.lock().map(|s| u32::from(s.rows)).unwrap_or(24)
    }

    pub fn styled_lines(&self) -> Vec<StyledLine> {
        self.snap
            .lock()
            .map(|s| s.lines.clone())
            .unwrap_or_default()
    }

    pub fn lines_json(&self) -> String {
        let lines = self.styled_lines();
        let mut out = String::from("[");
        for (li, line) in lines.iter().enumerate() {
            if li > 0 {
                out.push(',');
            }
            out.push('[');
            for (si, span) in line.spans.iter().enumerate() {
                if si > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"text\":{},\"fg\":{},\"bg\":{},\"bold\":{},\"italic\":{},\"underline\":{},\"blink\":{},\"strike\":{}}}",
                    json_str(&span.text),
                    span.fg,
                    span.bg,
                    span.bold,
                    span.italic,
                    span.underline,
                    span.blink,
                    span.strike
                ));
            }
            out.push(']');
        }
        out.push(']');
        out
    }

    pub fn write_viewport(&self, origin: u32, out: &mut [WwnTermCell]) -> (u32, u32, u32) {
        let _ = self.tx.send(Cmd::ScrollToOrigin(origin));
        self.sync_refresh();
        let Ok(s) = self.snap.lock() else {
            return (0, 0, 0);
        };
        let cols = u32::from(s.cols);
        let rows = u32::from(s.rows);
        let need = (cols as usize).saturating_mul(rows as usize);
        let n = need.min(out.len()).min(s.cells.len());
        out[..n].copy_from_slice(&s.cells[..n]);
        (n as u32, cols, rows)
    }

    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let mut data = text.as_bytes().to_vec();
        let mut buf = vec![0u8; data.len().saturating_mul(2).saturating_add(16)];
        match paste::encode(&mut data, true, &mut buf) {
            Ok(n) => {
                buf.truncate(n);
                buf
            }
            Err(_) => text.as_bytes().to_vec(),
        }
    }

    pub fn placed_images(&self) -> Vec<PlacedImage> {
        Vec::new()
    }

    pub fn origin(&self) -> u32 {
        0
    }
}

impl Drop for VtEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn worker(rx: Receiver<Cmd>, snap: Arc<Mutex<Snapshot>>, cols: u16, rows: u16) {
    let Ok(mut terminal) = Terminal::new(cols, rows) else {
        return;
    };
    let _ = terminal.set_scrollback_max_lines(Some(DEFAULT_SCROLLBACK));

    let reply_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let reply_cb = Arc::clone(&reply_buf);
    let _ = terminal.on_pty_write(move |_term, data| {
        if let Ok(mut buf) = reply_cb.lock() {
            buf.extend_from_slice(data);
        }
    });

    let Ok(mut render_state) = RenderState::new() else {
        return;
    };
    let Ok(mut row_iter) = RowIterator::new() else {
        return;
    };
    let Ok(mut cell_iter) = CellIterator::new() else {
        return;
    };

    let mut cell_w = 8u32;
    let mut cell_h = 16u32;
    let mut scroll_cap = DEFAULT_SCROLLBACK;
    let mut mouse = libghostty_vt::mouse::Encoder::new().ok();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Shutdown => break,
            Cmd::Feed(bytes) => terminal.vt_write(&bytes),
            Cmd::Resize { cols, rows } => {
                let _ = terminal.resize(cols, rows, cell_w, cell_h);
                if let Ok(mut s) = snap.lock() {
                    s.cols = cols;
                    s.rows = rows;
                }
            }
            Cmd::SetCellPx { w, h } => {
                cell_w = w;
                cell_h = h;
                let (cols, rows) = snap
                    .lock()
                    .map(|s| (s.cols, s.rows))
                    .unwrap_or((80, 24));
                let _ = terminal.resize(cols, rows, cell_w, cell_h);
            }
            Cmd::SetScrollCap(cap) => {
                scroll_cap = cap;
                let _ = terminal.set_scrollback_max_lines(Some(cap));
            }
            Cmd::ScrollToOrigin(origin) => {
                if origin == 0 {
                    terminal.scroll_viewport(ScrollViewport::Bottom);
                } else {
                    terminal.scroll_viewport(ScrollViewport::Row(origin as usize));
                }
            }
            Cmd::Pointer {
                button,
                col,
                row,
                pressed,
                motion,
            } => {
                if let Some(enc) = mouse.as_mut() {
                    let _ = enc.set_options_from_terminal(&terminal);
                    let action = if motion {
                        libghostty_vt::mouse::Action::Motion
                    } else if pressed {
                        libghostty_vt::mouse::Action::Press
                    } else {
                        libghostty_vt::mouse::Action::Release
                    };
                    if let Ok(mut event) = libghostty_vt::mouse::Event::new() {
                        event.set_action(action);
                        event.set_button(Some(button_from_u8(button)));
                        // Tip MousePosition is cell-space f32.
                        event.set_position(libghostty_vt::mouse::Position {
                            x: col as f32,
                            y: row as f32,
                        });
                        let mut out = Vec::new();
                        if enc.encode_to_vec(&event, &mut out).is_ok() && !out.is_empty() {
                            if let Ok(mut buf) = reply_buf.lock() {
                                buf.extend_from_slice(&out);
                            }
                        }
                    }
                }
            }
            Cmd::Refresh { done } => {
                publish(
                    &snap,
                    &terminal,
                    &mut render_state,
                    &mut row_iter,
                    &mut cell_iter,
                    &reply_buf,
                    cell_w,
                    cell_h,
                    scroll_cap,
                );
                let _ = done.send(());
            }
        }
    }
}

fn button_from_u8(button: u8) -> libghostty_vt::mouse::Button {
    match button {
        2 => libghostty_vt::mouse::Button::Middle,
        3 => libghostty_vt::mouse::Button::Right,
        4 => libghostty_vt::mouse::Button::Four,
        5 => libghostty_vt::mouse::Button::Five,
        _ => libghostty_vt::mouse::Button::Left,
    }
}

fn publish<'a>(
    snap: &Arc<Mutex<Snapshot>>,
    terminal: &Terminal<'a, 'a>,
    render_state: &mut RenderState<'a>,
    row_iter: &mut RowIterator<'a>,
    cell_iter: &mut CellIterator<'a>,
    reply_buf: &Arc<Mutex<Vec<u8>>>,
    cell_w: u32,
    cell_h: u32,
    scroll_cap: usize,
) {
    let Ok(snapshot) = render_state.update(terminal) else {
        return;
    };
    let Ok(colors) = snapshot.colors() else {
        return;
    };
    let default_fg = pack_rgb(colors.foreground.r, colors.foreground.g, colors.foreground.b);
    let default_bg = pack_rgb(colors.background.r, colors.background.g, colors.background.b);
    let palette = colors.palette;

    let mut cells = Vec::new();
    let mut lines = Vec::new();
    let Ok(mut rows) = row_iter.update(&snapshot) else {
        return;
    };
    while let Some(row) = rows.next() {
        let Ok(mut cells_it) = cell_iter.update(&row) else {
            break;
        };
        let mut line = StyledLine::default();
        let mut cur: Option<StyledSpan> = None;
        while let Some(cell) = cells_it.next() {
            let Ok(graphemes) = cell.graphemes() else {
                continue;
            };
            let style = cell.style().unwrap_or_else(|_| Style::default());
            let mut fg = resolve_color(style.fg_color, default_fg, &palette);
            let mut bg = resolve_color(style.bg_color, default_bg, &palette);
            // SGR reverse video (fastfetch blocks, vim, …). Swap before paint.
            if style.inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            let mut attrs = 0u32;
            if style.bold {
                attrs |= ATTR_BOLD;
            }
            if style.faint {
                attrs |= ATTR_DIM;
            }
            if style.italic {
                attrs |= ATTR_ITALIC;
            }
            if style.underline != Underline::None {
                attrs |= ATTR_UNDERLINE;
            }
            if style.blink {
                attrs |= ATTR_BLINK;
            }
            if style.strikethrough {
                attrs |= ATTR_STRIKE;
            }
            let cp = if style.invisible {
                b' ' as u32
            } else {
                graphemes.first().copied().unwrap_or(' ') as u32
            };
            cells.push(WwnTermCell { cp, fg, bg, attrs });

            let text: String = if graphemes.is_empty() || style.invisible {
                " ".into()
            } else {
                graphemes.iter().collect()
            };
            let same = cur.as_ref().is_some_and(|s| {
                s.fg == fg
                    && s.bg == bg
                    && s.bold == style.bold
                    && s.italic == style.italic
                    && s.underline == (style.underline != Underline::None)
                    && s.blink == style.blink
                    && s.strike == style.strikethrough
            });
            if same {
                if let Some(s) = cur.as_mut() {
                    s.text.push_str(&text);
                }
            } else {
                if let Some(s) = cur.take() {
                    line.spans.push(s);
                }
                cur = Some(StyledSpan {
                    text,
                    fg,
                    bg,
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline != Underline::None,
                    blink: style.blink,
                    strike: style.strikethrough,
                });
            }
        }
        if let Some(s) = cur.take() {
            line.spans.push(s);
        }
        lines.push(line);
        let _ = row.set_dirty(false);
    }
    let _ = snapshot.set_dirty(Dirty::Clean);

    let title = terminal
        .title()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();
    let reply = reply_buf
        .lock()
        .map(|mut b| std::mem::take(&mut *b))
        .unwrap_or_default();

    // Prefer last Resize size so the Metal grid stays cols×rows (pad blanks).
    let (cols, rows_n) = snap
        .lock()
        .map(|s| (s.cols.max(1), s.rows.max(1)))
        .unwrap_or((80, 24));
    let need = (cols as usize).saturating_mul(rows_n as usize);
    if cells.len() < need {
        cells.resize(
            need,
            WwnTermCell {
                cp: b' ' as u32,
                fg: default_fg,
                bg: default_bg,
                attrs: 0,
            },
        );
    } else if cells.len() > need {
        cells.truncate(need);
    }
    while lines.len() < rows_n as usize {
        lines.push(StyledLine::default());
    }
    if lines.len() > rows_n as usize {
        lines.truncate(rows_n as usize);
    }

    let cursor_visible = terminal.is_cursor_visible().unwrap_or(true);
    let cursor_col = terminal.cursor_x().unwrap_or(0);
    let cursor_row = terminal.cursor_y().unwrap_or(0);
    if cursor_visible {
        let idx = (cursor_row as usize).saturating_mul(cols as usize)
            .saturating_add(cursor_col as usize);
        if let Some(cell) = cells.get_mut(idx) {
            cell.attrs |= ATTR_CURSOR;
            // Block cursor: invert like classic Ghostty block when cell has no reverse yet.
            std::mem::swap(&mut cell.fg, &mut cell.bg);
        }
    }

    if let Ok(mut s) = snap.lock() {
        s.cols = cols;
        s.rows = rows_n;
        s.cells = cells;
        s.lines = lines;
        s.title = title;
        if !reply.is_empty() {
            s.reply.extend_from_slice(&reply);
        }
        s.scroll_cap = scroll_cap;
        s.cell_w = cell_w;
        s.cell_h = cell_h;
        s.cursor_col = cursor_col;
        s.cursor_row = cursor_row;
        s.cursor_visible = cursor_visible;
        s.gen = s.gen.wrapping_add(1);
    }
}

fn resolve_color(color: StyleColor, default: u32, palette: &[RgbColor; 256]) -> u32 {
    match color {
        StyleColor::None => default,
        StyleColor::Rgb(RgbColor { r, g, b }) => pack_rgb(r, g, b),
        StyleColor::Palette(idx) => {
            let i = usize::try_from(u32::from(idx.0)).unwrap_or(usize::MAX);
            if i < palette.len() {
                let c = palette[i];
                pack_rgb(c.r, c.g, c.b)
            } else {
                default
            }
        }
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Recommend scrollback rows from host RAM.
pub fn recommended_scroll_cap(cols: u32) -> usize {
    let cols = cols.max(1) as usize;
    let bytes_per_row = cols.saturating_mul(16).max(1);
    let avail = available_memory_bytes().unwrap_or(512 * 1024 * 1024) as usize;
    let budget = (avail / 64).max(64 * 1024);
    (budget / bytes_per_row).clamp(1_000, 100_000)
}

fn available_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut len = std::mem::size_of::<u64>();
        let mut name = [6i32, 24i32]; // CTL_HW, HW_MEMSIZE
        let mut out = std::mem::MaybeUninit::<u64>::uninit();
        let rc = unsafe {
            sysctl(
                name.as_mut_ptr(),
                2,
                out.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            Some(unsafe { out.assume_init() })
        } else {
            None
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn sysctl(
        name: *mut i32,
        namelen: u32,
        oldp: *mut std::ffi::c_void,
        oldlenp: *mut usize,
        newp: *mut std::ffi::c_void,
        newlen: usize,
    ) -> i32;
}
