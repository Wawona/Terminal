//! VT screen for Wawona Terminal.
//!
//! Hosts upload the viewport and draw it on the GPU. Kitty graphics are
//! RGBA placements the host blits. This file does not link a GPU library.



const SCROLL_CAP: usize = 8_000;
const COLS_MAX: usize = 512;
const ROWS_MAX: usize = 256;

const ATTR_BOLD: u32 = 1;
const ATTR_DIM: u32 = 2;
const ATTR_ITALIC: u32 = 4;
const ATTR_UNDERLINE: u32 = 8;
const ATTR_BLINK: u32 = 16;
const ATTR_STRIKE: u32 = 32;
const APC_CAP: usize = 8 * 1024 * 1024;

#[path = "kitty.rs"]
mod kitty;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    DefaultFg,
    DefaultBg,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy)]
struct Style {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    strike: bool,
    reverse: bool,
}

impl Style {
    fn reset() -> Self {
        Self {
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            blink: false,
            strike: false,
            reverse: false,
        }
    }
}

#[derive(Clone, Copy)]
struct Cell {
    cp: char,
    fg: u32,
    bg: u32,
    attrs: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[repr(C)]
pub struct WwnTermCell {
    pub cp: u32,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u32,
}

pub(crate) struct PlacedImage {
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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WwnTermImage {
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
    pub rgba_len: u32,
}

struct Alt {
    screen: Vec<Cell>,
    cursor_x: usize,
    cursor_y: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Vt {
    Ground,
    Esc,
    EscInt,
    Csi,
    Osc,
    OscEsc,
    Apc,
    ApcEsc,
}

pub struct Term {
    cols: usize,
    rows: usize,
    screen: Vec<Cell>,
    scrollback: Vec<Vec<Cell>>,
    cursor_x: usize,
    cursor_y: usize,
    pending_wrap: bool,
    /// DEC autowrap (mode 7). On by default. A graphic past the last column
    /// wraps once. A following CR or LF does not add a second break.
    autowrap: bool,
    style: Style,
    alt: Option<Alt>,
    state: Vt,
    csi_private: bool,
    csi_params: Vec<u16>,
    csi_current: u16,
    csi_has: bool,
    csi_after_semi: bool,
    utf8_need: u8,
    utf8_acc: u32,
    replies: Vec<u8>,
    saved_x: usize,
    saved_y: usize,
    /// Inclusive top row of the scrolling region.
    scroll_top: usize,
    /// Exclusive bottom row of the scrolling region.
    scroll_bottom: usize,
    cursor_visible: bool,
    insert_mode: bool,
    last_graphic: Option<char>,
    bracketed_paste: bool,
    mouse_button: bool,
    mouse_drag: bool,
    mouse_any: bool,
    mouse_sgr: bool,
    /// DEC origin mode (DECOM, private mode 6). CUP is relative to the scroll region.
    origin_mode: bool,
    osc_buf: Vec<u8>,
    apc_buf: Vec<u8>,
    title: String,
    kitty: kitty::Kitty,
}

fn indexed(i: u8) -> u32 {
    const BASE: [u32; 16] = [
        0x1e1e1e, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
        0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
    ];
    if i < 16 {
        return BASE[i as usize];
    }
    if i < 232 {
        let n = i - 16;
        let r = n / 36;
        let g = (n % 36) / 6;
        let b = n % 6;
        let c = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
        return (u32::from(c(r)) << 16) | (u32::from(c(g)) << 8) | u32::from(c(b));
    }
    let v = 8 + 10 * u32::from(i - 232);
    (v << 16) | (v << 8) | v
}

fn resolve(color: Color) -> u32 {
    match color {
        Color::DefaultFg => 0xE8E8E8,
        Color::DefaultBg => 0x000000,
        Color::Indexed(i) => indexed(i),
        Color::Rgb(r, g, b) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
    }
}

/// Display columns. Private-use Nerd icons stay one cell. CJK and emoji are two.
fn display_width(ch: char) -> usize {
    let u = ch as u32;
    if (0xE000..=0xF8FF).contains(&u)
        || (0xF0000..=0xFFFFD).contains(&u)
        || (0x100000..=0x10FFFD).contains(&u)
    {
        return 1;
    }
    if (0x1100..=0x115F).contains(&u)
        || (0x2329..=0x232A).contains(&u)
        || ((0x2E80..=0xA4CF).contains(&u) && u != 0x303F)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE10..=0xFE19).contains(&u)
        || (0xFE30..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1F64F).contains(&u)
        || (0x1F900..=0x1F9FF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
    {
        return 2;
    }
    1
}

fn dim_rgb(rgb: u32) -> u32 {
    let scale = |c: u32| (c * 62) / 100;
    (scale((rgb >> 16) & 0xff) << 16) | (scale((rgb >> 8) & 0xff) << 8) | scale(rgb & 0xff)
}

impl Term {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.clamp(1, COLS_MAX);
        let rows = rows.clamp(1, ROWS_MAX);
        let style = Style::reset();
        let blank = Self::blank_with(&style);
        Self {
            cols,
            rows,
            screen: vec![blank; cols * rows],
            scrollback: Vec::new(),
            cursor_x: 0,
            cursor_y: 0,
            pending_wrap: false,
            autowrap: true,
            style,
            alt: None,
            state: Vt::Ground,
            csi_private: false,
            csi_params: Vec::new(),
            csi_current: 0,
            csi_has: false,
            csi_after_semi: false,
            utf8_need: 0,
            utf8_acc: 0,
            replies: Vec::new(),
            saved_x: 0,
            saved_y: 0,
            scroll_top: 0,
            scroll_bottom: rows,
            cursor_visible: true,
            insert_mode: false,
            last_graphic: None,
            bracketed_paste: false,
            mouse_button: false,
            mouse_drag: false,
            mouse_any: false,
            mouse_sgr: false,
            origin_mode: false,
            osc_buf: Vec::new(),
            apc_buf: Vec::new(),
            title: String::new(),
            kitty: kitty::Kitty::new(),
        }
    }

    fn blank_with(style: &Style) -> Cell {
        let (fg, bg, _attrs) = paint(style);
        Cell {
            cp: ' ',
            fg,
            bg,
            attrs: 0,
        }
    }

    fn blank(&self) -> Cell {
        Self::blank_with(&self.style)
    }

    pub(crate) fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.clamp(1, COLS_MAX);
        let rows = rows.clamp(1, ROWS_MAX);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let old_cols = self.cols;
        let old_rows = self.rows;
        let old = std::mem::replace(&mut self.screen, Vec::new());
        self.cols = cols;
        self.rows = rows;
        self.screen = vec![self.blank(); cols * rows];
        let drop = old_rows.saturating_sub(rows);
        if self.alt.is_none() {
            for y in 0..drop {
                let line = old[y * old_cols..(y + 1) * old_cols].to_vec();
                self.push_scrollback(line);
            }
        }
        let copy_rows = old_rows.saturating_sub(drop).min(rows);
        let copy_cols = old_cols.min(cols);
        for y in 0..copy_rows {
            for x in 0..copy_cols {
                self.screen[y * cols + x] = old[(drop + y) * old_cols + x];
            }
        }
        self.cursor_y = self.cursor_y.saturating_sub(drop).min(rows - 1);
        self.cursor_x = self.cursor_x.min(cols - 1);
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = rows;
        if drop > 0 {
            self.kitty.scroll(0, old_rows, drop, true, rows);
        } else {
            self.kitty.scroll(0, rows, 0, true, rows);
        }
    }

    fn push_scrollback(&mut self, mut line: Vec<Cell>) {
        while line
            .last()
            .is_some_and(|c| c.cp == ' ' && c.bg == 0 && c.attrs == 0)
        {
            line.pop();
        }
        self.scrollback.push(line);
        if self.scrollback.len() > SCROLL_CAP {
            let extra = self.scrollback.len() - SCROLL_CAP;
            self.scrollback.drain(0..extra);
        }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn cols(&self) -> usize {
        self.cols
    }

    #[allow(dead_code)]
    pub(crate) fn drain_reply(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Bytes to write to the PTY for a paste. Mode 2004 wraps the payload.
    pub(crate) fn encode_paste(&self, text: &str) -> Vec<u8> {
        if !self.bracketed_paste {
            return text.as_bytes().to_vec();
        }
        let mut out = Vec::with_capacity(text.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    }

    /// Pointer event. `button` is 0 left, 1 middle, 2 right. Motion is ignored
    /// until mode 1002 or 1003 is on. Encoding is SGR when mode 1006 is on.
    pub(crate) fn pointer(&mut self, button: u8, col: u32, row: u32, pressed: bool, motion: bool) {
        let tracking = if motion {
            self.mouse_any || (self.mouse_drag && pressed)
        } else {
            self.mouse_button
        };
        if !tracking {
            return;
        }
        let col = col.max(1);
        let row = row.max(1);
        let code = if !pressed && !self.mouse_sgr {
            3u32
        } else if motion {
            u32::from(button.min(2)) + 32
        } else {
            u32::from(button.min(2))
        };
        if self.mouse_sgr {
            let end = if pressed { 'M' } else { 'm' };
            self.replies
                .extend(format!("\x1b[<{code};{col};{row}{end}").into_bytes());
        } else {
            let cb = (code as u8).saturating_add(32);
            let cx = (col as u8).saturating_add(32);
            let cy = (row as u8).saturating_add(32);
            self.replies
                .extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
        }
    }

    fn byte(&mut self, b: u8) {
        if self.utf8_need > 0 && !matches!(self.state, Vt::Apc | Vt::ApcEsc) {
            if b & 0xC0 == 0x80 {
                self.utf8_acc = (self.utf8_acc << 6) | u32::from(b & 0x3F);
                self.utf8_need -= 1;
                if self.utf8_need == 0 {
                    let cp = self.utf8_acc;
                    self.utf8_acc = 0;
                    if let Some(ch) = char::from_u32(cp) {
                        self.print_char(ch);
                    }
                }
                return;
            }
            self.utf8_need = 0;
        }
        match self.state {
            Vt::Ground => self.ground(b),
            Vt::Esc => self.esc(b),
            Vt::EscInt => {
                if (0x30..=0x7E).contains(&b) {
                    self.state = Vt::Ground;
                }
            }
            Vt::Csi => self.csi(b),
            Vt::Osc => {
                if b == 0x07 {
                    self.finish_osc();
                    self.state = Vt::Ground;
                } else if b == 0x1B {
                    self.state = Vt::OscEsc;
                } else if self.osc_buf.len() < 512 {
                    self.osc_buf.push(b);
                }
            }
            Vt::OscEsc => {
                if b == b'\\' {
                    self.finish_osc();
                    self.state = Vt::Ground;
                } else {
                    if self.osc_buf.len() < 512 {
                        self.osc_buf.push(0x1B);
                        self.osc_buf.push(b);
                    }
                    self.state = Vt::Osc;
                }
            }
            Vt::Apc => {
                if b == 0x1B {
                    self.state = Vt::ApcEsc;
                } else if self.apc_buf.len() >= APC_CAP {
                    self.apc_buf.clear();
                    self.kitty.abort();
                    self.replies.extend_from_slice(b"\x1b_G;ENOSPC\x1b\\");
                    self.state = Vt::Ground;
                } else {
                    self.apc_buf.push(b);
                }
            }
            Vt::ApcEsc => {
                if b == b'\\' {
                    self.finish_apc();
                    self.state = Vt::Ground;
                } else {
                    if self.apc_buf.len() + 2 < APC_CAP {
                        self.apc_buf.push(0x1B);
                        self.apc_buf.push(b);
                    }
                    self.state = Vt::Apc;
                }
            }
        }
    }

    fn ground(&mut self, b: u8) {
        match b {
            0x1B => self.state = Vt::Esc,
            0x00 | 0x7F => {}
            b if b < 0x20 => self.control(b),
            b if b < 0x80 => self.print_char(b as char),
            b if b & 0xE0 == 0xC0 => {
                self.utf8_need = 1;
                self.utf8_acc = u32::from(b & 0x1F);
            }
            b if b & 0xF0 == 0xE0 => {
                self.utf8_need = 2;
                self.utf8_acc = u32::from(b & 0x0F);
            }
            b if b & 0xF8 == 0xF0 => {
                self.utf8_need = 3;
                self.utf8_acc = u32::from(b & 0x07);
            }
            _ => {}
        }
    }

    fn control(&mut self, b: u8) {
        match b {
            // LF and FF are index: down one row, same column. CR is separate.
            // ncurses `ind` / `cud1` is LF. A new row at column 0 is CR+LF
            // (the PTY ONLCR path, or NEL).
            b'\n' | 0x0C => self.index(),
            b'\r' => self.carriage_return(),
            0x08 => {
                self.pending_wrap = false;
                if self.cursor_x == 0 {
                    return;
                }
                self.cursor_x -= 1;
                if self.cursor_x > 0 {
                    let idx = self.cursor_y * self.cols + self.cursor_x;
                    if self.screen[idx].cp == '\0' {
                        self.cursor_x -= 1;
                    }
                }
            }
            b'\t' => {
                let next = (self.cursor_x / 8 + 1) * 8;
                self.cursor_x = next.min(self.cols - 1);
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn esc(&mut self, b: u8) {
        match b {
            b'[' => {
                self.csi_private = false;
                self.csi_params.clear();
                self.csi_current = 0;
                self.csi_has = false;
                self.csi_after_semi = false;
                self.state = Vt::Csi;
            }
            b']' => {
                self.osc_buf.clear();
                self.state = Vt::Osc;
            }
            b'_' => {
                self.apc_buf.clear();
                self.state = Vt::Apc;
            }
            b'7' => {
                self.saved_x = self.cursor_x;
                self.saved_y = self.cursor_y;
                self.state = Vt::Ground;
            }
            b'8' => {
                self.cursor_x = self.saved_x.min(self.cols - 1);
                self.cursor_y = self.saved_y.min(self.rows - 1);
                self.state = Vt::Ground;
            }
            b'D' => {
                self.index();
                self.state = Vt::Ground;
            }
            b'M' => {
                self.reverse_index();
                self.state = Vt::Ground;
            }
            b'E' => {
                self.newline();
                self.state = Vt::Ground;
            }
            b'c' => {
                self.reset_hard();
                self.state = Vt::Ground;
            }
            0x20..=0x2F => self.state = Vt::EscInt,
            _ => self.state = Vt::Ground,
        }
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                let digit = u16::from(b - b'0');
                self.csi_current = self.csi_current.saturating_mul(10).saturating_add(digit);
                self.csi_has = true;
                self.csi_after_semi = false;
            }
            b';' => {
                self.csi_params
                    .push(if self.csi_has { self.csi_current } else { 0 });
                self.csi_current = 0;
                self.csi_has = false;
                self.csi_after_semi = true;
            }
            b'?' if self.csi_params.is_empty() && !self.csi_has => {
                self.csi_private = true;
            }
            0x40..=0x7E => {
                if self.csi_has || self.csi_params.is_empty() || self.csi_after_semi {
                    self.csi_params
                        .push(if self.csi_has { self.csi_current } else { 0 });
                }
                let params = std::mem::take(&mut self.csi_params);
                let private = self.csi_private;
                self.state = Vt::Ground;
                self.dispatch(b, private, &params);
            }
            0x20..=0x2F => {}
            _ => self.state = Vt::Ground,
        }
    }

    fn dispatch(&mut self, final_byte: u8, private: bool, params: &[u16]) {
        let p0 = params.first().copied().unwrap_or(0);
        let n = |v: u16| usize::from(if v == 0 { 1 } else { v });
        match final_byte {
            b'm' if !private => self.sgr(params),
            b'A' if !private => self.move_vertical(-(n(p0) as isize)),
            b'B' if !private => self.move_vertical(n(p0) as isize),
            b'C' if !private => {
                self.pending_wrap = false;
                self.cursor_x = (self.cursor_x + n(p0)).min(self.cols - 1);
            }
            b'D' if !private => {
                self.pending_wrap = false;
                self.cursor_x = self.cursor_x.saturating_sub(n(p0));
            }
            b'G' if !private => {
                self.pending_wrap = false;
                self.cursor_x = n(p0).saturating_sub(1).min(self.cols - 1);
            }
            b'd' if !private => {
                self.pending_wrap = false;
                let row = n(p0);
                if self.origin_mode {
                    let top = self.region_top();
                    let bottom = self.region_bottom().saturating_sub(1);
                    self.cursor_y = (top + row.saturating_sub(1)).min(bottom);
                } else {
                    self.cursor_y = row.saturating_sub(1).min(self.rows - 1);
                }
            }
            b'H' | b'f' if !private => {
                let row = n(params.first().copied().unwrap_or(1));
                let col = n(params.get(1).copied().unwrap_or(1));
                self.pending_wrap = false;
                self.cursor_x = col.saturating_sub(1).min(self.cols - 1);
                if self.origin_mode {
                    let top = self.region_top();
                    let bottom = self.region_bottom().saturating_sub(1);
                    self.cursor_y = (top + row.saturating_sub(1)).min(bottom);
                } else {
                    self.cursor_y = row.saturating_sub(1).min(self.rows - 1);
                }
            }
            b'J' if !private => self.erase_display(p0),
            b'K' if !private => self.erase_line(p0),
            b'h' | b'l' => self.set_mode(private, params, final_byte == b'h'),
            b'L' if !private => self.insert_lines(n(p0)),
            b'M' if !private => self.delete_lines(n(p0)),
            b'@' if !private => self.insert_blanks(n(p0)),
            b'P' if !private => self.delete_chars(n(p0)),
            b'X' if !private => self.erase_chars(n(p0)),
            b'S' if !private => self.scroll_region(n(p0), true),
            b'T' if !private => self.scroll_region(n(p0), false),
            b'E' if !private => {
                self.pending_wrap = false;
                for _ in 0..n(p0) {
                    self.index();
                }
                self.cursor_x = 0;
            }
            b'F' if !private => {
                self.pending_wrap = false;
                for _ in 0..n(p0) {
                    if self.cursor_y == 0 {
                        break;
                    }
                    self.cursor_y -= 1;
                }
                self.cursor_x = 0;
            }
            b'b' if !private => {
                if let Some(ch) = self.last_graphic {
                    for _ in 0..n(p0) {
                        self.print_char(ch);
                    }
                }
            }
            b'r' if !private => self.set_scroll_region(params),
            b's' if !private => {
                self.saved_x = self.cursor_x;
                self.saved_y = self.cursor_y;
            }
            b'u' if !private => {
                self.cursor_x = self.saved_x.min(self.cols - 1);
                self.cursor_y = self.saved_y.min(self.rows - 1);
            }
            b'n' if !private => self.report(p0),
            b'c' if !private || private => {
                self.replies.extend_from_slice(b"\x1b[?6c");
            }
            _ => {}
        }
    }

    fn report(&mut self, mode: u16) {
        if mode == 6 {
            let row = self.cursor_y + 1;
            let col = self.cursor_x + 1;
            self.replies
                .extend(format!("\x1b[{row};{col}R").into_bytes());
        } else if mode == 5 {
            self.replies.extend_from_slice(b"\x1b[0n");
        }
    }

    fn set_mode(&mut self, private: bool, params: &[u16], enable: bool) {
        if !private {
            for mode in params {
                if *mode == 4 {
                    self.insert_mode = enable;
                }
            }
            return;
        }
        for mode in params {
            if *mode == 6 {
                self.origin_mode = enable;
                self.pending_wrap = false;
                self.cursor_x = 0;
                self.cursor_y = if enable { self.region_top() } else { 0 };
            }
            if *mode == 7 {
                self.autowrap = enable;
                if !enable {
                    self.pending_wrap = false;
                }
            }
            if *mode == 25 {
                self.cursor_visible = enable;
            }
            if *mode == 1000 {
                self.mouse_button = enable;
            }
            if *mode == 1002 {
                self.mouse_button = enable;
                self.mouse_drag = enable;
            }
            if *mode == 1003 {
                self.mouse_button = enable;
                self.mouse_drag = enable;
                self.mouse_any = enable;
            }
            if *mode == 1006 {
                self.mouse_sgr = enable;
            }
            if *mode == 2004 {
                self.bracketed_paste = enable;
            }
            if matches!(mode, 47 | 1047 | 1049) {
                if enable {
                    self.enter_alt();
                } else {
                    self.leave_alt();
                }
            }
        }
    }

    fn enter_alt(&mut self) {
        if self.alt.is_some() {
            return;
        }
        self.alt = Some(Alt {
            screen: self.screen.clone(),
            cursor_x: self.cursor_x,
            cursor_y: self.cursor_y,
        });
        self.clear_screen();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows;
        self.kitty.set_alt(true);
    }

    fn leave_alt(&mut self) {
        if let Some(alt) = self.alt.take() {
            self.screen = alt.screen;
            self.cursor_x = alt.cursor_x.min(self.cols - 1);
            self.cursor_y = alt.cursor_y.min(self.rows - 1);
            self.pending_wrap = false;
            self.scroll_top = 0;
            self.scroll_bottom = self.rows;
            self.kitty.set_alt(false);
        }
    }

    fn sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.style = Style::reset();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.style = Style::reset(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                5 | 6 => self.style.blink = true,
                7 => self.style.reverse = true,
                9 => self.style.strike = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                25 => self.style.blink = false,
                27 => self.style.reverse = false,
                29 => self.style.strike = false,
                39 => self.style.fg = Color::DefaultFg,
                49 => self.style.bg = Color::DefaultBg,
                30..=37 => self.style.fg = Color::Indexed((p - 30) as u8),
                40..=47 => self.style.bg = Color::Indexed((p - 40) as u8),
                90..=97 => self.style.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.style.bg = Color::Indexed((p - 100 + 8) as u8),
                38 | 48 => {
                    let fg = p == 38;
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        let color = Color::Indexed(params[i + 2] as u8);
                        if fg {
                            self.style.fg = color;
                        } else {
                            self.style.bg = color;
                        }
                        i += 3;
                        continue;
                    }
                    if i + 4 < params.len() && params[i + 1] == 2 {
                        let color = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        if fg {
                            self.style.fg = color;
                        } else {
                            self.style.bg = color;
                        }
                        i += 5;
                        continue;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn print_char(&mut self, ch: char) {
        let width = display_width(ch);
        if self.pending_wrap {
            if self.autowrap {
                self.newline();
            } else {
                self.pending_wrap = false;
            }
        }
        if width == 2 && self.cursor_x + 1 >= self.cols {
            if self.autowrap {
                self.newline();
            } else {
                return;
            }
        }
        if self.insert_mode {
            self.insert_blanks(width);
        }
        self.last_graphic = Some(ch);
        self.put(self.cursor_x, self.cursor_y, ch);
        if width == 2 {
            let x = self.cursor_x + 1;
            let (fg, bg, attrs) = paint(&self.style);
            self.screen[self.cursor_y * self.cols + x] = Cell {
                cp: '\0',
                fg,
                bg,
                attrs,
            };
        }
        let next = self.cursor_x + width;
        if next >= self.cols {
            if self.autowrap {
                self.pending_wrap = true;
            }
            self.cursor_x = self.cols - 1;
        } else {
            self.cursor_x = next;
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        self.break_wide(x, y);
        let (fg, bg, attrs) = paint(&self.style);
        self.screen[y * self.cols + x] = Cell {
            cp: ch,
            fg,
            bg,
            attrs,
        };
    }

    fn break_wide(&mut self, x: usize, y: usize) {
        let idx = y * self.cols + x;
        let cell = self.screen[idx];
        let blank = Cell {
            cp: ' ',
            fg: cell.fg,
            bg: cell.bg,
            attrs: 0,
        };
        if cell.cp == '\0' && x > 0 {
            self.screen[idx - 1] = blank;
        } else if display_width(cell.cp) == 2 && x + 1 < self.cols {
            self.screen[idx + 1] = blank;
        }
    }

    fn carriage_return(&mut self) {
        self.pending_wrap = false;
        self.cursor_x = 0;
    }

    fn index(&mut self) {
        self.pending_wrap = false;
        let bottom = self.region_bottom();
        if self.cursor_y + 1 >= bottom {
            self.scroll_region(1, true);
        } else {
            self.cursor_y += 1;
        }
    }

    fn newline(&mut self) {
        self.carriage_return();
        self.index();
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.cursor_y <= self.region_top() {
            self.scroll_region(1, false);
        } else {
            self.cursor_y -= 1;
        }
    }

    fn region_top(&self) -> usize {
        self.scroll_top.min(self.rows.saturating_sub(1))
    }

    fn region_bottom(&self) -> usize {
        self.scroll_bottom.clamp(self.region_top() + 1, self.rows)
    }

    fn full_region(&self) -> bool {
        self.region_top() == 0 && self.region_bottom() == self.rows
    }

    fn set_scroll_region(&mut self, params: &[u16]) {
        let top = usize::from(params.first().copied().unwrap_or(1).max(1));
        let bottom = usize::from(params.get(1).copied().unwrap_or(self.rows as u16).max(1));
        if top < bottom && bottom <= self.rows {
            self.scroll_top = top - 1;
            self.scroll_bottom = bottom;
        }
        self.pending_wrap = false;
        self.cursor_x = 0;
        self.cursor_y = self.scroll_top;
    }

    /// Scroll the active region. `up` moves rows toward the top.
    fn scroll_region(&mut self, count: usize, up: bool) {
        let top = self.region_top();
        let bottom = self.region_bottom();
        let height = bottom - top;
        let count = count.min(height);
        if count == 0 {
            return;
        }
        let cols = self.cols;
        let blank = self.blank();
        for _ in 0..count {
            if up {
                if self.alt.is_none() && self.full_region() {
                    let row = self.screen[top * cols..(top + 1) * cols].to_vec();
                    self.push_scrollback(row);
                }
                let start = top * cols;
                let end = bottom * cols;
                self.screen.copy_within(start + cols..end, start);
                for cell in &mut self.screen[end - cols..end] {
                    *cell = blank;
                }
            } else {
                let start = top * cols;
                let end = bottom * cols;
                self.screen.copy_within(start..end - cols, start + cols);
                for cell in &mut self.screen[start..start + cols] {
                    *cell = blank;
                }
            }
        }
        self.kitty.scroll(top, bottom, count, up, self.rows);
    }

    fn insert_lines(&mut self, count: usize) {
        let top = self.region_top();
        let bottom = self.region_bottom();
        let y = self.cursor_y;
        if y < top || y >= bottom || count == 0 {
            return;
        }
        let count = count.min(bottom - y);
        let cols = self.cols;
        let dest = (y + count) * cols;
        let end = bottom * cols;
        if dest < end {
            self.screen.copy_within(y * cols..end - count * cols, dest);
        }
        self.clear_rows(y, y + count);
    }

    fn delete_lines(&mut self, count: usize) {
        let top = self.region_top();
        let bottom = self.region_bottom();
        let y = self.cursor_y;
        if y < top || y >= bottom || count == 0 {
            return;
        }
        let count = count.min(bottom - y);
        let cols = self.cols;
        let src = (y + count) * cols;
        let end = bottom * cols;
        self.screen.copy_within(src..end, y * cols);
        self.clear_rows(bottom - count, bottom);
    }

    fn insert_blanks(&mut self, count: usize) {
        let y = self.cursor_y;
        let x = self.cursor_x;
        if x >= self.cols || count == 0 {
            return;
        }
        let count = count.min(self.cols - x);
        let row = y * self.cols;
        self.screen
            .copy_within(row + x..row + self.cols - count, row + x + count);
        let blank = self.blank();
        for cell in &mut self.screen[row + x..row + x + count] {
            *cell = blank;
        }
    }

    fn delete_chars(&mut self, count: usize) {
        let y = self.cursor_y;
        let x = self.cursor_x;
        if x >= self.cols || count == 0 {
            return;
        }
        let count = count.min(self.cols - x);
        let row = y * self.cols;
        self.screen
            .copy_within(row + x + count..row + self.cols, row + x);
        let blank = self.blank();
        for cell in &mut self.screen[row + self.cols - count..row + self.cols] {
            *cell = blank;
        }
    }

    fn erase_chars(&mut self, count: usize) {
        let y = self.cursor_y;
        let x = self.cursor_x;
        if x >= self.cols || count == 0 {
            return;
        }
        let end = (x + count).min(self.cols);
        let blank = self.blank();
        for cell in &mut self.screen[y * self.cols + x..y * self.cols + end] {
            *cell = blank;
        }
    }

    fn clear_rows(&mut self, start: usize, end: usize) {
        let blank = self.blank();
        let from = start.min(self.rows) * self.cols;
        let to = end.min(self.rows) * self.cols;
        for cell in &mut self.screen[from..to] {
            *cell = blank;
        }
    }

    fn clear_screen(&mut self) {
        let blank = self.blank();
        for cell in &mut self.screen {
            *cell = blank;
        }
    }

    fn archive_screen(&mut self) {
        for y in 0..self.rows {
            let line = self.screen[y * self.cols..(y + 1) * self.cols].to_vec();
            if line.iter().any(|c| c.cp != ' ') {
                self.push_scrollback(line);
            }
        }
    }

    fn erase_display(&mut self, mode: u16) {
        match mode {
            0 => {
                self.erase_line(0);
                for y in (self.cursor_y + 1)..self.rows {
                    self.clear_row(y);
                }
            }
            1 => {
                for y in 0..self.cursor_y {
                    self.clear_row(y);
                }
                self.erase_line(1);
            }
            2 | 3 => {
                if self.alt.is_none() && mode == 2 {
                    self.archive_screen();
                }
                if mode == 3 {
                    self.scrollback.clear();
                }
                self.clear_screen();
                self.cursor_x = 0;
                self.cursor_y = 0;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let y = self.cursor_y;
        let start = match mode {
            0 => self.cursor_x,
            1 => 0,
            2 => 0,
            _ => return,
        };
        let end = match mode {
            0 | 2 => self.cols,
            1 => self.cursor_x + 1,
            _ => return,
        };
        let blank = self.blank();
        for x in start..end {
            self.screen[y * self.cols + x] = blank;
        }
    }

    fn clear_row(&mut self, y: usize) {
        let blank = self.blank();
        for x in 0..self.cols {
            self.screen[y * self.cols + x] = blank;
        }
    }

    fn reset_hard(&mut self) {
        self.style = Style::reset();
        self.alt = None;
        self.clear_screen();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.pending_wrap = false;
        self.autowrap = true;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows;
        self.cursor_visible = true;
        self.insert_mode = false;
        self.last_graphic = None;
        self.bracketed_paste = false;
        self.mouse_button = false;
        self.mouse_drag = false;
        self.mouse_any = false;
        self.mouse_sgr = false;
        self.origin_mode = false;
        self.osc_buf.clear();
        self.apc_buf.clear();
        self.title.clear();
        self.kitty.clear();
        self.state = Vt::Ground;
    }

    fn finish_apc(&mut self) {
        let buf = std::mem::take(&mut self.apc_buf);
        let outcome = self.kitty.handle(&buf, self.cursor_x, self.cursor_y, self.alt.is_some());
        if !outcome.reply.is_empty() {
            self.replies.extend_from_slice(&outcome.reply);
        }
        if let Some(row) = outcome.cursor_row {
            self.cursor_after_image(row);
        }
    }

    fn cursor_after_image(&mut self, row: usize) {
        self.pending_wrap = false;
        self.cursor_x = 0;
        if row < self.rows {
            self.cursor_y = row;
            return;
        }
        let extra = row - (self.rows - 1);
        for _ in 0..extra {
            self.cursor_y = self.rows - 1;
            self.index();
        }
        self.cursor_x = 0;
    }

    /// CUU / CUD. Inside the scrolling region the cursor stops at the margins.
    fn move_vertical(&mut self, delta: isize) {
        self.pending_wrap = false;
        let top = self.region_top();
        let bottom = self.region_bottom().saturating_sub(1);
        let inside = self.cursor_y >= top && self.cursor_y <= bottom;
        let next = if delta < 0 {
            self.cursor_y.saturating_sub(delta.unsigned_abs())
        } else {
            self.cursor_y.saturating_add(delta as usize)
        };
        self.cursor_y = if inside {
            next.clamp(top, bottom)
        } else {
            next.min(self.rows.saturating_sub(1))
        };
    }

    fn finish_osc(&mut self) {
        let raw = std::mem::take(&mut self.osc_buf);
        let text = String::from_utf8_lossy(&raw);
        let body = if let Some(rest) = text.strip_prefix("0;") {
            rest
        } else if let Some(rest) = text.strip_prefix("2;") {
            rest
        } else {
            return;
        };
        self.title = body.chars().take(240).collect();
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn set_cell_px(&mut self, w: u32, h: u32) {
        self.kitty.set_cell_px(w, h);
    }

    pub(crate) fn images_json(&self) -> String {
        self.kitty.images_json()
    }

    pub(crate) fn image_count(&self) -> u32 {
        self.kitty.len() as u32
    }

    pub(crate) fn placed_images(&self) -> Vec<PlacedImage> {
        (0..self.kitty.len())
            .filter_map(|index| {
                let image = self.kitty.view(index)?;
                Some(PlacedImage {
                    id: image.id,
                    col: image.col,
                    row: image.row,
                    x_px: image.x_px,
                    y_px: image.y_px,
                    px_w: image.px_w,
                    px_h: image.px_h,
                    cols: image.cols,
                    rows: image.rows,
                    z: image.z,
                    rgba: image.rgba.to_vec(),
                })
            })
            .collect()
    }

    fn used_rows(&self) -> usize {
        let mut last = 0;
        for y in (0..self.rows).rev() {
            let row = &self.screen[y * self.cols..(y + 1) * self.cols];
            if row.iter().any(|c| c.cp != ' ' || c.bg != 0) {
                last = y;
                break;
            }
        }
        last.max(self.cursor_y) + 1
    }

    pub(crate) fn line_count(&self) -> usize {
        self.scrollback.len() + self.used_rows()
    }

    pub(crate) fn line_cells(&self, index: usize, out: &mut [WwnTermCell]) -> usize {
        let cols = self.cols.min(out.len());
        if index >= self.line_count() || cols == 0 {
            return 0;
        }
        let blank = Cell {
            cp: ' ',
            fg: 0xE8E8E8,
            bg: 0,
            attrs: 0,
        };
        if index < self.scrollback.len() {
            let line = &self.scrollback[index];
            for x in 0..cols {
                let cell = line.get(x).copied().unwrap_or(blank);
                out[x] = export_cell(cell);
            }
        } else {
            let y = index - self.scrollback.len();
            for x in 0..cols {
                out[x] = export_cell(self.screen[y * self.cols + x]);
            }
            if self.cursor_visible && y == self.cursor_y && self.cursor_x < cols {
                let cell = &mut out[self.cursor_x];
                std::mem::swap(&mut cell.fg, &mut cell.bg);
                if cell.bg == 0 {
                    cell.bg = 0xE8E8E8;
                    if cell.fg == 0xE8E8E8 {
                        cell.fg = 0;
                    }
                }
            }
        }
        cols
    }

    fn write_view(&self, origin: usize, out: &mut [WwnTermCell]) {
        let cols = self.cols;
        let rows = self.rows;
        let blank = export_cell(Cell {
            cp: ' ',
            fg: 0xE8E8E8,
            bg: 0,
            attrs: 0,
        });
        for y in 0..rows {
            let index = origin + y;
            let row = &mut out[y * cols..(y + 1) * cols];
            if index < self.scrollback.len() {
                let line = &self.scrollback[index];
                for x in 0..cols {
                    row[x] = line.get(x).copied().map(export_cell).unwrap_or(blank);
                }
            } else {
                let sy = index - self.scrollback.len();
                if sy < self.rows {
                    for x in 0..cols {
                        row[x] = export_cell(self.screen[sy * cols + x]);
                    }
                    if self.cursor_visible && sy == self.cursor_y && self.cursor_x < cols {
                        let cell = &mut row[self.cursor_x];
                        std::mem::swap(&mut cell.fg, &mut cell.bg);
                        if cell.bg == 0 {
                            cell.bg = 0xE8E8E8;
                            if cell.fg == 0xE8E8E8 {
                                cell.fg = 0;
                            }
                        }
                    }
                } else {
                    row.fill(blank);
                }
            }
        }
    }
}

fn paint(style: &Style) -> (u32, u32, u32) {
    let mut fg = resolve(style.fg);
    let mut bg = resolve(style.bg);
    if style.reverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if style.dim {
        fg = dim_rgb(fg);
    }
    let mut attrs = 0;
    if style.bold {
        attrs |= ATTR_BOLD;
    }
    if style.dim {
        attrs |= ATTR_DIM;
    }
    if style.italic {
        attrs |= ATTR_ITALIC;
    }
    if style.underline {
        attrs |= ATTR_UNDERLINE;
    }
    if style.blink {
        attrs |= ATTR_BLINK;
    }
    if style.strike {
        attrs |= ATTR_STRIKE;
    }
    (fg, bg, attrs)
}

fn export_cell(cell: Cell) -> WwnTermCell {
    WwnTermCell {
        cp: cell.cp as u32,
        fg: cell.fg,
        bg: cell.bg,
        attrs: cell.attrs,
    }
}

fn term_mut(term: *mut Term) -> Option<&'static mut Term> {
    if term.is_null() {
        None
    } else {
        Some(unsafe { &mut *term })
    }
}

fn term_ref<'a>(term: *const Term) -> Option<&'a Term> {
    if term.is_null() {
        None
    } else {
        Some(unsafe { &*term })
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_new(cols: u32, rows: u32) -> *mut Term {
    Box::into_raw(Box::new(Term::new(cols as usize, rows as usize)))
}

#[no_mangle]
pub extern "C" fn wwn_term_free(term: *mut Term) {
    if !term.is_null() {
        drop(unsafe { Box::from_raw(term) });
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_resize(term: *mut Term, cols: u32, rows: u32) {
    if let Some(term) = term_mut(term) {
        term.resize(cols as usize, rows as usize);
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_feed(term: *mut Term, bytes: *const u8, len: usize) {
    if bytes.is_null() && len > 0 {
        return;
    }
    if let Some(term) = term_mut(term) {
        let slice = if len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes, len) }
        };
        term.feed(slice);
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_take_reply(term: *mut Term, buf: *mut u8, cap: usize) -> usize {
    let Some(term) = term_mut(term) else {
        return 0;
    };
    if buf.is_null() || cap == 0 {
        return term.replies.len();
    }
    let n = term.replies.len().min(cap);
    unsafe { std::ptr::copy_nonoverlapping(term.replies.as_ptr(), buf, n) };
    term.replies.drain(..n);
    n
}

#[no_mangle]
pub extern "C" fn wwn_term_cols(term: *const Term) -> u32 {
    term_ref(term).map(|t| t.cols as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn wwn_term_line_count(term: *const Term) -> u32 {
    term_ref(term).map(|t| t.line_count() as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn wwn_term_line_cells(
    term: *const Term,
    line: u32,
    out: *mut WwnTermCell,
    cap: u32,
) -> u32 {
    if out.is_null() || cap == 0 {
        return 0;
    }
    let Some(term) = term_ref(term) else {
        return 0;
    };
    let slice = unsafe { std::slice::from_raw_parts_mut(out, cap as usize) };
    term.line_cells(line as usize, slice) as u32
}

#[no_mangle]
pub extern "C" fn wwn_term_cursor(term: *const Term, line: *mut u32, col: *mut u32) {
    let Some(term) = term_ref(term) else {
        return;
    };
    if !line.is_null() {
        unsafe { *line = (term.scrollback.len() + term.cursor_y) as u32 };
    }
    if !col.is_null() {
        unsafe { *col = term.cursor_x as u32 };
    }
}

/// One styled run. Hosts draw this. They do not group cells.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// One visual row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

impl Term {
    /// Spans for every row, including scrollback. The cursor cell is already inverted.
    pub fn styled_lines(&self) -> Vec<StyledLine> {
        let count = self.line_count();
        let mut lines = Vec::with_capacity(count);
        for index in 0..count {
            lines.push(StyledLine {
                spans: spans_for(self, index),
            });
        }
        lines
    }

    /// JSON document of [`styled_lines`]. Same fields as the UniFFI records.
    pub fn lines_json(&self) -> String {
        let lines = self.styled_lines();
        let mut out = String::from("[");
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"spans\":[");
            for (j, span) in line.spans.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                out.push_str("{\"text\":");
                push_json_string(&mut out, &span.text);
                out.push_str(&format!(
                    ",\"fg\":{},\"bg\":{},\"bold\":{},\"italic\":{},\"underline\":{},\"blink\":{},\"strike\":{}}}",
                    span.fg,
                    span.bg,
                    span.bold,
                    span.italic,
                    span.underline,
                    span.blink,
                    span.strike
                ));
            }
            out.push_str("]}");
        }
        out.push(']');
        out
    }
}

fn spans_for(term: &Term, index: usize) -> Vec<StyledSpan> {
    let cols = term.cols();
    if cols == 0 {
        return Vec::new();
    }
    let mut cells = vec![
        WwnTermCell {
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
            push_styled(&mut spans, &mut text, style);
            style = Some(key);
        }
        if cell.cp == 0 {
            continue;
        }
        let cp = char::from_u32(cell.cp).unwrap_or(' ');
        text.push(cp);
    }
    push_styled(&mut spans, &mut text, style);
    spans
}

fn push_styled(spans: &mut Vec<StyledSpan>, text: &mut String, style: Option<(u32, u32, u32)>) {
    if text.is_empty() {
        return;
    }
    let (fg, bg, attrs) = style.unwrap_or((0xE8E8E8, 0, 0));
    spans.push(StyledSpan {
        text: std::mem::take(text),
        fg,
        bg,
        bold: attrs & 1 != 0,
        italic: attrs & 4 != 0,
        underline: attrs & 8 != 0,
        blink: attrs & ATTR_BLINK != 0,
        strike: attrs & ATTR_STRIKE != 0,
    });
}

fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn leak_c_string(text: String) -> *mut std::os::raw::c_char {
    std::ffi::CString::new(text)
        .map(std::ffi::CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Visible grid for one GPU upload. `origin` is a scrollback row. The live
/// screen starts at `scrollback.len()`.
#[no_mangle]
pub extern "C" fn wwn_term_viewport(
    term: *const Term,
    origin: u32,
    out: *mut WwnTermCell,
    cap: u32,
    cols_out: *mut u32,
    rows_out: *mut u32,
) -> u32 {
    let Some(term) = term_ref(term) else {
        return 0;
    };
    if out.is_null() || cols_out.is_null() || rows_out.is_null() {
        return 0;
    }
    let cols = term.cols;
    let rows = term.rows;
    let need = cols * rows;
    unsafe {
        *cols_out = cols as u32;
        *rows_out = rows as u32;
    }
    if (cap as usize) < need {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(out, need) };
    term.write_view(origin as usize, slice);
    need as u32
}

#[no_mangle]
pub extern "C" fn wwn_term_origin(term: *const Term) -> u32 {
    term_ref(term)
        .map(|term| term.scrollback.len() as u32)
        .unwrap_or(0)
}

/// OSC 0 / OSC 2 title, NUL-terminated. Returns the byte length without the NUL.
#[no_mangle]
pub extern "C" fn wwn_term_title(term: *const Term, out: *mut u8, cap: u32) -> u32 {
    let Some(term) = term_ref(term) else {
        return 0;
    };
    if out.is_null() || cap == 0 {
        return 0;
    }
    let bytes = term.title.as_bytes();
    let n = bytes.len().min((cap as usize).saturating_sub(1));
    unsafe {
        if n > 0 {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n);
        }
        *out.add(n) = 0;
    }
    n as u32
}

#[no_mangle]
pub extern "C" fn wwn_term_set_cell_px(term: *mut Term, width: u32, height: u32) {
    if let Some(term) = term_mut(term) {
        term.set_cell_px(width, height);
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_image_count(term: *const Term) -> u32 {
    term_ref(term).map(|term| term.image_count()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn wwn_term_image_meta(term: *const Term, index: u32, out: *mut WwnTermImage) -> i32 {
    let Some(term) = term_ref(term) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    let Some(image) = term.kitty.view(index as usize) else {
        return 0;
    };
    unsafe {
        *out = WwnTermImage {
            id: image.id,
            col: image.col,
            row: image.row,
            x_px: image.x_px,
            y_px: image.y_px,
            px_w: image.px_w,
            px_h: image.px_h,
            cols: image.cols,
            rows: image.rows,
            z: image.z,
            rgba_len: image.rgba.len() as u32,
        };
    }
    1
}

#[no_mangle]
pub extern "C" fn wwn_term_image_rgba(term: *const Term, index: u32, out: *mut u8, cap: u32) -> u32 {
    let Some(term) = term_ref(term) else {
        return 0;
    };
    let Some(image) = term.kitty.view(index as usize) else {
        return 0;
    };
    if out.is_null() || (cap as usize) < image.rgba.len() {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(image.rgba.as_ptr(), out, image.rgba.len());
    }
    image.rgba.len() as u32
}

#[no_mangle]
pub extern "C" fn wwn_term_images_json(term: *const Term) -> *mut std::os::raw::c_char {
    let Some(term) = term_ref(term) else {
        return std::ptr::null_mut();
    };
    leak_c_string(term.images_json())
}

#[no_mangle]
pub extern "C" fn wwn_term_bytes_images_json(
    bytes: *const u8,
    len: usize,
    cols: u32,
    rows: u32,
    cell_w: u32,
    cell_h: u32,
) -> *mut std::os::raw::c_char {
    let slice = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    let mut term = Term::new(cols as usize, rows as usize);
    term.set_cell_px(cell_w, cell_h);
    term.feed(slice);
    leak_c_string(term.images_json())
}

#[no_mangle]
pub extern "C" fn wwn_term_lines_json(term: *const Term) -> *mut std::os::raw::c_char {
    let Some(term) = term_ref(term) else {
        return std::ptr::null_mut();
    };
    leak_c_string(term.lines_json())
}

#[no_mangle]
pub extern "C" fn wwn_term_bytes_lines_json(
    bytes: *const u8,
    len: usize,
    cols: u32,
    rows: u32,
) -> *mut std::os::raw::c_char {
    let slice = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    let mut term = Term::new(cols as usize, rows as usize);
    term.feed(slice);
    leak_c_string(term.lines_json())
}

#[no_mangle]
pub extern "C" fn wwn_term_encode_paste(
    term: *const Term,
    bytes: *const u8,
    len: usize,
) -> *mut std::os::raw::c_char {
    let Some(term) = term_ref(term) else {
        return std::ptr::null_mut();
    };
    let slice = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    let text = String::from_utf8_lossy(slice);
    leak_c_string(String::from_utf8_lossy(&term.encode_paste(&text)).into_owned())
}

#[no_mangle]
pub extern "C" fn wwn_term_pointer(
    term: *mut Term,
    button: u8,
    col: u32,
    row: u32,
    pressed: i32,
    motion: i32,
) {
    if let Some(term) = term_mut(term) {
        term.pointer(button, col, row, pressed != 0, motion != 0);
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_string_free(ptr: *mut std::os::raw::c_char) {
    if !ptr.is_null() {
        drop(unsafe { std::ffi::CString::from_raw(ptr) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_lines_are_the_host_document() {
        let mut term = Term::new(8, 2);
        term.feed(b"hi");
        let lines = term.styled_lines();
        assert!(lines[0].spans.iter().any(|span| span.text.contains("hi")));
        let json = term.lines_json();
        assert!(json.contains("\"text\":\"hi\""));
        assert!(json.contains("\"bold\":false"));
    }

    fn line_text(term: &Term, index: usize) -> (String, Vec<u32>) {
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0
            };
            term.cols
        ];
        let n = term.line_cells(index, &mut cells);
        let mut text = String::new();
        let mut attrs = Vec::new();
        for cell in cells.iter().take(n) {
            if cell.cp == 0 {
                continue;
            }
            text.push(char::from_u32(cell.cp).unwrap_or(' '));
            attrs.push(cell.attrs);
        }
        while text.ends_with(' ') {
            text.pop();
            attrs.pop();
        }
        (text, attrs)
    }

    #[test]
    fn bold_green_is_a_style_not_plaintext() {
        let mut term = Term::new(80, 24);
        term.feed(b"\x1b[1;32mOK\x1b[0m");
        let (text, attrs) = line_text(&term, 0);
        assert_eq!(text, "OK");
        assert!(attrs.iter().all(|a| a & ATTR_BOLD != 0));
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0
            };
            8
        ];
        term.line_cells(0, &mut cells);
        assert_eq!(cells[0].fg, indexed(2));
        assert!(!text.contains('['));
    }

    #[test]
    fn truecolor_and_nerd_icon_survive() {
        let mut term = Term::new(40, 8);
        let icon = "\u{F0A0}";
        let raw = format!("\x1b[38;2;10;20;30m{icon}\x1b[0m");
        term.feed(raw.as_bytes());
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0
            };
            4
        ];
        term.line_cells(0, &mut cells);
        assert_eq!(cells[0].cp, 0xF0A0);
        assert_eq!(cells[0].fg, (10 << 16) | (20 << 8) | 30);
    }

    #[test]
    fn newline_scrolls_into_history() {
        let mut term = Term::new(8, 2);
        term.feed(b"one\r\ntwo\r\nthree");
        assert!(term.line_count() >= 3);
        let (first, _) = line_text(&term, 0);
        assert_eq!(first, "one");
        let (last, _) = line_text(&term, term.line_count() - 1);
        assert_eq!(last, "three");
    }

    #[test]
    fn line_feed_keeps_the_column() {
        let mut term = Term::new(8, 4);
        term.feed(b"abcd\nX");
        let (first, _) = line_text(&term, 0);
        let (second, _) = line_text(&term, 1);
        assert_eq!(first, "abcd");
        assert_eq!(second, "    X");
    }

    #[test]
    fn carriage_return_line_feed_starts_the_next_row() {
        let mut term = Term::new(8, 4);
        term.feed(b"abcd\r\nX");
        let (second, _) = line_text(&term, 1);
        assert_eq!(second, "X");
        assert_eq!(term.line_count(), 2);
    }

    #[test]
    fn full_width_line_then_crlf_does_not_insert_a_blank_row() {
        let mut term = Term::new(4, 6);
        term.feed(b"abcd\r\nX");
        let (first, _) = line_text(&term, 0);
        let (second, _) = line_text(&term, 1);
        assert_eq!(first, "abcd");
        assert_eq!(second, "X");
        assert_eq!(term.line_count(), 2);
    }

    #[test]
    fn blank_row_between_text_stays_in_the_grid() {
        let mut term = Term::new(8, 6);
        term.feed(b"top\r\n\r\nbottom");
        assert_eq!(term.line_count(), 3);
        let (mid, _) = line_text(&term, 1);
        let (last, _) = line_text(&term, 2);
        assert_eq!(mid, "");
        assert_eq!(last, "bottom");
    }

    #[test]
    fn insert_and_delete_chars_shift_the_row() {
        let mut term = Term::new(8, 2);
        term.feed(b"ABCD\x1b[3D\x1b[2P");
        let (text, _) = line_text(&term, 0);
        assert_eq!(text, "AD");
        term.feed(b"\x1b[1@Z");
        let (text, _) = line_text(&term, 0);
        assert_eq!(text, "AZD");
    }

    #[test]
    fn insert_line_pushes_the_row_down() {
        let mut term = Term::new(8, 4);
        term.feed(b"one\x1b[H\x1b[Ltwo");
        let (first, _) = line_text(&term, 0);
        let (second, _) = line_text(&term, 1);
        assert_eq!(first, "two");
        assert_eq!(second, "one");
    }

    #[test]
    fn scroll_region_keeps_the_row_above_it() {
        let mut term = Term::new(8, 4);
        term.feed(b"top\r\n\x1b[2;4r\x1b[4;1Hmid\r\nbot");
        let (first, _) = line_text(&term, 0);
        assert_eq!(first, "top");
        let (last, _) = line_text(&term, term.line_count() - 1);
        assert_eq!(last, "bot");
    }

    #[test]
    fn cursor_inverts_the_cell_under_it() {
        let mut term = Term::new(8, 2);
        term.feed(b"A\x08");
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0,
            };
            4
        ];
        term.line_cells(0, &mut cells);
        assert_eq!(cells[0].cp, u32::from(b'A'));
        assert_eq!(cells[0].bg, 0xE8E8E8);
        assert_eq!(cells[0].fg, 0);
        term.feed(b"\x1b[?25l");
        term.line_cells(0, &mut cells);
        assert_eq!(cells[0].fg, 0xE8E8E8);
        assert_eq!(cells[0].bg, 0);
    }

    #[test]
    fn viewport_is_the_live_grid() {
        let mut term = Term::new(4, 2);
        term.feed(b"ab");
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0,
            };
            8
        ];
        term.write_view(term.scrollback.len(), &mut cells);
        assert_eq!(cells[0].cp, u32::from(b'a'));
        assert_eq!(cells[1].cp, u32::from(b'b'));
        assert_eq!(cells[4].cp, u32::from(b' '));
    }

    #[test]
    fn blink_and_strike_are_span_flags() {
        let mut term = Term::new(16, 2);
        term.feed(b"\x1b[5;9mHi\x1b[0m");
        let spans = &term.styled_lines()[0].spans;
        let span = spans.iter().find(|span| span.text.contains("Hi")).unwrap();
        assert!(span.blink);
        assert!(span.strike);
        assert!(term.lines_json().contains("\"blink\":true"));
        assert!(term.lines_json().contains("\"strike\":true"));
    }

    #[test]
    fn bracketed_paste_wraps_only_when_enabled() {
        let mut term = Term::new(16, 2);
        assert_eq!(term.encode_paste("ab"), b"ab");
        term.feed(b"\x1b[?2004h");
        assert_eq!(term.encode_paste("ab"), b"\x1b[200~ab\x1b[201~");
        term.feed(b"\x1b[?2004l");
        assert_eq!(term.encode_paste("ab"), b"ab");
    }

    #[test]
    fn sgr_mouse_reports_press_and_ignores_motion_until_asked() {
        let mut term = Term::new(16, 4);
        term.pointer(0, 2, 3, true, false);
        assert!(term.drain_reply().is_empty());
        term.feed(b"\x1b[?1000h\x1b[?1006h");
        term.pointer(0, 2, 3, true, false);
        assert_eq!(term.drain_reply(), b"\x1b[<0;2;3M");
        term.pointer(0, 2, 3, true, true);
        assert!(term.drain_reply().is_empty());
        term.feed(b"\x1b[?1003h");
        term.pointer(0, 4, 1, true, true);
        assert_eq!(term.drain_reply(), b"\x1b[<32;4;1M");
    }

    #[test]
    fn wide_han_takes_two_columns_and_nerd_icon_stays_one() {
        let mut term = Term::new(8, 2);
        term.feed("中".as_bytes());
        assert_eq!(term.cols(), 8);
        let (text, _) = line_text(&term, 0);
        assert_eq!(text, "中");
        term.feed(b"\x08\x08");
        term.feed("\u{e0b0}".as_bytes());
        let (text, _) = line_text(&term, 0);
        assert!(text.contains('\u{e0b0}'));
        let mut cells = vec![
            WwnTermCell {
                cp: 0,
                fg: 0,
                bg: 0,
                attrs: 0,
            };
            4
        ];
        term.line_cells(0, &mut cells);
        assert_eq!(cells[0].cp, u32::from('\u{e0b0}'));
        assert_ne!(cells[1].cp, 0);
    }

    #[test]
    fn cursor_up_stops_at_the_scroll_margin() {
        let mut term = Term::new(10, 6);
        term.feed(b"\x1b[3;5r\x1b[4;1H\x1b[10A");
        assert_eq!(term.cursor_y, 2);
    }

    #[test]
    fn origin_mode_addresses_inside_the_region() {
        let mut term = Term::new(10, 6);
        term.feed(b"\x1b[3;5r\x1b[?6h\x1b[1;1HZ");
        assert_eq!(term.cursor_y, 2);
        let (text, _) = line_text(&term, 2);
        assert!(text.starts_with('Z'));
    }

    #[test]
    fn osc_title_is_kept() {
        let mut term = Term::new(8, 2);
        term.feed(b"\x1b]2;Wawona\x07");
        assert_eq!(term.title(), "Wawona");
        term.feed(b"\x1bc");
        assert_eq!(term.title(), "");
    }

    #[test]
    fn kitty_rgba_places_pixels_and_does_not_print_the_payload() {
        let mut term = Term::new(8, 4);
        term.feed(b"\x1b_Ga=T,f=32,s=1,v=1,i=7;/wAA/w==\x1b\\");
        assert_eq!(term.image_count(), 1);
        let image = term.kitty.view(0).unwrap();
        assert_eq!(image.rgba, &[255, 0, 0, 255]);
        assert_eq!(image.col, 0);
        assert_eq!(image.row, 0);
        let (text, _) = line_text(&term, 0);
        assert!(!text.contains('/'));
        assert!(term.drain_reply().windows(2).any(|pair| pair == b"OK"));
    }

    #[test]
    fn kitty_chunks_reassemble_and_delete_clears() {
        let mut term = Term::new(8, 2);
        term.feed(b"\x1b_Ga=T,f=32,s=1,v=1,i=3,m=1;/wAA\x1b\\");
        assert_eq!(term.image_count(), 0);
        term.feed(b"\x1b_Gm=0;/w==\x1b\\");
        assert_eq!(term.kitty.view(0).unwrap().rgba, &[255, 0, 0, 255]);
        term.feed(b"\x1b_Ga=d,d=i,i=3\x1b\\");
        assert_eq!(term.image_count(), 0);
    }

    #[test]
    fn kitty_query_ok_and_file_transmission_is_refused() {
        let mut term = Term::new(8, 2);
        term.feed(b"\x1b_Ga=q\x1b\\");
        assert_eq!(term.drain_reply(), b"\x1b_Ga=q;OK\x1b\\");
        term.feed(b"\x1b_Ga=T,t=f,f=32,s=1,v=1;/wAA/w==\x1b\\");
        assert_eq!(term.image_count(), 0);
        assert!(term.drain_reply().windows(6).any(|chunk| chunk == b"EINVAL"));
    }

    #[test]
    fn kitty_png_and_scroll_drop_a_placement() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 255, 0, 255]).unwrap();
        }
        let mut payload = String::new();
        kitty::b64_encode(&png, &mut payload);
        let mut term = Term::new(8, 2);
        term.feed(format!("\x1b_Ga=T,f=100,i=1;{payload}\x1b\\").as_bytes());
        assert_eq!(term.kitty.view(0).unwrap().rgba, &[0, 255, 0, 255]);
        term.feed(b"\n\n");
        assert_eq!(term.image_count(), 0);
    }
}
