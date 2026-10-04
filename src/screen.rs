//! VT screen for Wawona Terminal.
//!
//! Hosts draw the cells. This file does not own SwiftUI, Compose, or GTK.
//! The face is the bundled DejaVu Sans Mono Nerd Font. Apple system fonts
//! do not include those icons.

use std::alloc::{alloc, dealloc, Layout};

const SCROLL_CAP: usize = 8_000;
const COLS_MAX: usize = 512;
const ROWS_MAX: usize = 256;

const ATTR_BOLD: u32 = 1;
const ATTR_DIM: u32 = 2;
const ATTR_ITALIC: u32 = 4;
const ATTR_UNDERLINE: u32 = 8;

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
pub struct WwnTermCell {
    pub cp: u32,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u32,
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
}

pub struct Term {
    cols: usize,
    rows: usize,
    screen: Vec<Cell>,
    scrollback: Vec<Vec<Cell>>,
    cursor_x: usize,
    cursor_y: usize,
    pending_wrap: bool,
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
}

fn indexed(i: u8) -> u32 {
    const BASE: [u32; 16] = [
        0x1e1e1e, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf,
        0x555753, 0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
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

    fn byte(&mut self, b: u8) {
        if self.utf8_need > 0 {
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
                    self.state = Vt::Ground;
                } else if b == 0x1B {
                    self.state = Vt::OscEsc;
                }
            }
            Vt::OscEsc => {
                self.state = if b == b'\\' { Vt::Ground } else { Vt::Osc };
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
            b'\n' | b'\x0C' => self.newline(),
            b'\r' => self.carriage_return(),
            0x08 => {
                self.pending_wrap = false;
                self.cursor_x = self.cursor_x.saturating_sub(1);
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
            b']' => self.state = Vt::Osc,
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
            b'A' if !private => {
                self.pending_wrap = false;
                self.cursor_y = self.cursor_y.saturating_sub(n(p0));
            }
            b'B' if !private => {
                self.pending_wrap = false;
                self.cursor_y = (self.cursor_y + n(p0)).min(self.rows - 1);
            }
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
                self.cursor_y = n(p0).saturating_sub(1).min(self.rows - 1);
            }
            b'H' | b'f' if !private => {
                let row = n(params.first().copied().unwrap_or(1));
                let col = n(params.get(1).copied().unwrap_or(1));
                self.pending_wrap = false;
                self.cursor_y = row.saturating_sub(1).min(self.rows - 1);
                self.cursor_x = col.saturating_sub(1).min(self.cols - 1);
            }
            b'J' if !private => self.erase_display(p0),
            b'K' if !private => self.erase_line(p0),
            b'h' | b'l' => self.set_mode(private, params, final_byte == b'h'),
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
            return;
        }
        for mode in params {
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
    }

    fn leave_alt(&mut self) {
        if let Some(alt) = self.alt.take() {
            self.screen = alt.screen;
            self.cursor_x = alt.cursor_x.min(self.cols - 1);
            self.cursor_y = alt.cursor_y.min(self.rows - 1);
            self.pending_wrap = false;
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
                7 => self.style.reverse = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.reverse = false,
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
        if self.pending_wrap {
            self.newline();
        }
        self.put(self.cursor_x, self.cursor_y, ch);
        if self.cursor_x + 1 >= self.cols {
            self.pending_wrap = true;
        } else {
            self.cursor_x += 1;
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char) {
        let (fg, bg, attrs) = paint(&self.style);
        self.screen[y * self.cols + x] = Cell { cp: ch, fg, bg, attrs };
    }

    fn carriage_return(&mut self) {
        self.pending_wrap = false;
        self.cursor_x = 0;
    }

    fn index(&mut self) {
        self.pending_wrap = false;
        if self.cursor_y + 1 >= self.rows {
            self.scroll_up();
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
        if self.cursor_y == 0 {
            self.scroll_down();
        } else {
            self.cursor_y -= 1;
        }
    }

    fn scroll_up(&mut self) {
        let row = self.screen[..self.cols].to_vec();
        self.screen.copy_within(self.cols.., 0);
        let start = (self.rows - 1) * self.cols;
        let blank = self.blank();
        for cell in &mut self.screen[start..] {
            *cell = blank;
        }
        if self.alt.is_none() {
            self.push_scrollback(row);
        }
    }

    fn scroll_down(&mut self) {
        let cols = self.cols;
        let len = self.screen.len();
        self.screen.copy_within(0..len - cols, cols);
        let blank = self.blank();
        for cell in &mut self.screen[..cols] {
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
        }
        cols
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

fn pack_words(term: &Term) -> Vec<u32> {
    let lines = term.line_count();
    let mut words = Vec::with_capacity(1 + lines * (1 + term.cols * 4));
    words.push(lines as u32);
    let mut row = vec![
        WwnTermCell {
            cp: 0,
            fg: 0,
            bg: 0,
            attrs: 0,
        };
        term.cols
    ];
    for line in 0..lines {
        let n = term.line_cells(line, &mut row);
        words.push(n as u32);
        for cell in row.iter().take(n) {
            words.push(cell.cp);
            words.push(cell.fg);
            words.push(cell.bg);
            words.push(cell.attrs);
        }
    }
    words
}

fn alloc_words(words: &[u32], out_len: *mut usize) -> *mut u8 {
    let bytes_len = words.len() * 4;
    if bytes_len == 0 || out_len.is_null() {
        return std::ptr::null_mut();
    }
    let layout = match Layout::from_size_align(bytes_len, 4) {
        Ok(layout) => layout,
        Err(_) => return std::ptr::null_mut(),
    };
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(words.as_ptr() as *const u8, ptr, bytes_len);
        *out_len = bytes_len;
    }
    ptr
}

#[no_mangle]
pub extern "C" fn wwn_term_snapshot(
    bytes: *const u8,
    len: usize,
    cols: u32,
    rows: u32,
    out_len: *mut usize,
) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { *out_len = 0 };
    let slice = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    let mut term = Term::new(cols as usize, rows as usize);
    term.feed(slice);
    alloc_words(&pack_words(&term), out_len)
}

#[no_mangle]
pub extern "C" fn wwn_term_export(term: *const Term, out_len: *mut usize) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { *out_len = 0 };
    let Some(term) = term_ref(term) else {
        return std::ptr::null_mut();
    };
    alloc_words(&pack_words(term), out_len)
}

#[no_mangle]
pub extern "C" fn wwn_term_snapshot_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(len, 4) {
        unsafe { dealloc(ptr, layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        term.feed(b"one\ntwo\nthree");
        assert!(term.line_count() >= 3);
        let (first, _) = line_text(&term, 0);
        assert_eq!(first, "one");
        let (last, _) = line_text(&term, term.line_count() - 1);
        assert_eq!(last, "three");
    }
}
