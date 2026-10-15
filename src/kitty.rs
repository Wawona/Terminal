//! Kitty graphics protocol (APC `ESC _ G`).
//!
//! Direct RGBA, RGB, and PNG become tightly packed RGBA8 placements.
//! File and shared-memory transmission are refused. This module does not
//! link a GPU library. Hosts blit the RGBA.

use std::io::{Cursor, Read};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_CHUNK: usize = 12 * 1024 * 1024;
const MAX_EDGE: u32 = 8192;

struct Image {
    id: u32,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

struct Placement {
    image_id: u32,
    placement_id: u32,
    col: i32,
    row: i32,
    x_px: i32,
    y_px: i32,
    px_w: u32,
    px_h: u32,
    cols: u32,
    rows: u32,
    z: i32,
    auto_c: bool,
    auto_r: bool,
    on_alt: bool,
}

#[derive(Clone)]
struct Keys {
    action: u8,
    format: u32,
    medium: u8,
    width: u32,
    height: u32,
    image_id: u32,
    has_id: bool,
    placement_id: u32,
    more: u32,
    compression: u8,
    quiet: u32,
    has_quiet: bool,
    cursor_move: u32,
    x: i32,
    y: i32,
    x_px: i32,
    y_px: i32,
    columns: u32,
    rows: u32,
    z: i32,
    delete: u8,
}

impl Keys {
    fn new() -> Self {
        Self {
            action: b't',
            format: 32,
            medium: b'd',
            width: 0,
            height: 0,
            image_id: 0,
            has_id: false,
            placement_id: 0,
            more: 0,
            compression: 0,
            quiet: 0,
            has_quiet: false,
            cursor_move: 0,
            x: 0,
            y: 0,
            x_px: 0,
            y_px: 0,
            columns: 0,
            rows: 0,
            z: 0,
            delete: b'a',
        }
    }
}

pub(crate) struct Outcome {
    pub reply: Vec<u8>,
    pub cursor_row: Option<usize>,
}

pub(crate) struct ImageView<'a> {
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
    pub rgba: &'a [u8],
}

pub(crate) struct Kitty {
    images: Vec<Image>,
    placements: Vec<Placement>,
    next_id: u32,
    cell_w: u32,
    cell_h: u32,
    showing_alt: bool,
    assembling: bool,
    pending: Keys,
    pending_b64: Vec<u8>,
}

impl Kitty {
    pub(crate) fn new() -> Self {
        Self {
            images: Vec::new(),
            placements: Vec::new(),
            next_id: 1,
            cell_w: 8,
            cell_h: 16,
            showing_alt: false,
            assembling: false,
            pending: Keys::new(),
            pending_b64: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.images.clear();
        self.placements.clear();
        self.showing_alt = false;
        self.abort();
    }

    pub(crate) fn abort(&mut self) {
        self.assembling = false;
        self.pending_b64.clear();
    }

    pub(crate) fn set_alt(&mut self, on: bool) {
        if self.showing_alt && !on {
            self.placements.retain(|p| !p.on_alt);
            self.drop_orphans();
        }
        self.showing_alt = on;
    }

    pub(crate) fn set_cell_px(&mut self, w: u32, h: u32) {
        if w > 0 {
            self.cell_w = w;
        }
        if h > 0 {
            self.cell_h = h;
        }
        for placement in &mut self.placements {
            if placement.auto_c {
                placement.cols = div_ceil(placement.px_w, self.cell_w).max(1);
            }
            if placement.auto_r {
                placement.rows = div_ceil(placement.px_h, self.cell_h).max(1);
            }
        }
    }

    pub(crate) fn scroll(&mut self, top: usize, bottom: usize, count: usize, up: bool, screen_rows: usize) {
        let top = top as i32;
        let bottom = bottom as i32;
        let count = count as i32;
        let showing = self.showing_alt;
        for placement in &mut self.placements {
            if placement.on_alt != showing {
                continue;
            }
            if placement.row >= top && placement.row < bottom {
                placement.row += if up { -count } else { count };
            }
        }
        let limit = screen_rows as i32;
        self.placements.retain(|placement| {
            placement.on_alt != showing || (placement.row < limit && placement.row + placement.rows as i32 > 0)
        });
    }

    pub(crate) fn handle(&mut self, apc: &[u8], cursor_x: usize, cursor_y: usize, on_alt: bool) -> Outcome {
        // APC graphics is `ESC _ G <control> ; <payload> ST`. Other APC is ignored.
        let Some(b'G') = apc.first() else {
            return Outcome { reply: Vec::new(), cursor_row: None };
        };
        let (ctrl, payload) = split_ctrl(&apc[1..]);
        let keys = parse_keys(ctrl);
        if self.assembling {
            return self.continue_chunk(&keys, payload, cursor_x, cursor_y, on_alt);
        }
        match keys.action {
            b'q' => self.query(&keys),
            b'd' => self.delete(&keys, cursor_x, cursor_y),
            b'p' => self.put(&keys, cursor_x, cursor_y, on_alt),
            b't' | b'T' => self.transmit(keys, payload, cursor_x, cursor_y, on_alt),
            _ => Outcome {
                reply: reply(keys.quiet, false, "", "EINVAL"),
                cursor_row: None,
            },
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.order().len()
    }

    pub(crate) fn view(&self, index: usize) -> Option<ImageView<'_>> {
        let slot = *self.order().get(index)?;
        let placement = &self.placements[slot];
        let image = self.images.iter().find(|image| image.id == placement.image_id)?;
        Some(ImageView {
            id: placement.image_id,
            col: placement.col,
            row: placement.row,
            x_px: placement.x_px,
            y_px: placement.y_px,
            px_w: image.w,
            px_h: image.h,
            cols: placement.cols,
            rows: placement.rows,
            z: placement.z,
            rgba: &image.rgba,
        })
    }

    pub(crate) fn images_json(&self) -> String {
        let mut out = String::from("[");
        for (n, slot) in self.order().into_iter().enumerate() {
            let placement = &self.placements[slot];
            let Some(image) = self.images.iter().find(|image| image.id == placement.image_id) else {
                continue;
            };
            if out.len() > 1 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"n\":{n},\"id\":{},\"col\":{},\"row\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"cols\":{},\"rows\":{},\"z\":{},\"rgba\":\"",
                placement.image_id,
                placement.col,
                placement.row,
                placement.x_px,
                placement.y_px,
                image.w,
                image.h,
                placement.cols,
                placement.rows,
                placement.z
            ));
            b64_encode(&image.rgba, &mut out);
            out.push_str("\"}");
        }
        out.push(']');
        out
    }

    fn continue_chunk(
        &mut self,
        keys: &Keys,
        payload: &[u8],
        cursor_x: usize,
        cursor_y: usize,
        on_alt: bool,
    ) -> Outcome {
        let extra = strip_ws(payload);
        if self.pending_b64.len().saturating_add(extra.len()) > MAX_CHUNK {
            self.abort();
            return Outcome {
                reply: reply(keys.quiet, false, "", "ENOSPC"),
                cursor_row: None,
            };
        }
        self.pending_b64.extend_from_slice(&extra);
        if keys.has_quiet {
            self.pending.quiet = keys.quiet;
        }
        if keys.more != 0 {
            return self.ok_reply();
        }
        self.assembling = false;
        self.finish(cursor_x, cursor_y, on_alt)
    }

    fn transmit(
        &mut self,
        mut keys: Keys,
        payload: &[u8],
        cursor_x: usize,
        cursor_y: usize,
        on_alt: bool,
    ) -> Outcome {
        if keys.medium != b'd' {
            return Outcome {
                reply: reply(keys.quiet, false, "", "EINVAL"),
                cursor_row: None,
            };
        }
        if !keys.has_id || keys.image_id == 0 {
            keys.image_id = self.alloc_id();
            keys.has_id = true;
        }
        self.pending = keys;
        self.pending_b64 = strip_ws(payload);
        if self.pending.more != 0 {
            self.assembling = true;
            return self.ok_reply();
        }
        self.finish(cursor_x, cursor_y, on_alt)
    }

    fn finish(&mut self, cursor_x: usize, cursor_y: usize, on_alt: bool) -> Outcome {
        let keys = self.pending.clone();
        let raw = match b64_decode(&self.pending_b64) {
            Some(bytes) => bytes,
            None => {
                self.pending_b64.clear();
                return Outcome {
                    reply: self.err("EINVAL"),
                    cursor_row: None,
                };
            }
        };
        self.pending_b64.clear();
        let data = if keys.compression == b'z' {
            match inflate_zlib(&raw) {
                Some(bytes) => bytes,
                None => {
                    return Outcome {
                        reply: self.err("EINVAL"),
                        cursor_row: None,
                    };
                }
            }
        } else {
            raw
        };
        let decoded = match keys.format {
            24 => rgb_to_rgba(&data, keys.width, keys.height),
            32 => rgba_exact(&data, keys.width, keys.height),
            100 => decode_png(&data),
            _ => None,
        };
        let Some((w, h, rgba)) = decoded else {
            return Outcome {
                reply: self.err("EINVAL"),
                cursor_row: None,
            };
        };
        if !self.store_image(keys.image_id, w, h, rgba) {
            return Outcome {
                reply: self.err("ENOSPC"),
                cursor_row: None,
            };
        }
        let mut cursor_row = None;
        if keys.action == b'T' {
            cursor_row = self.place(&keys, cursor_x, cursor_y, on_alt);
        }
        Outcome {
            reply: self.ok_reply().reply,
            cursor_row,
        }
    }

    fn put(&mut self, keys: &Keys, cursor_x: usize, cursor_y: usize, on_alt: bool) -> Outcome {
        if self.images.iter().all(|image| image.id != keys.image_id) {
            return Outcome {
                reply: reply(keys.quiet, false, &id_key(keys), "ENOENT"),
                cursor_row: None,
            };
        }
        let cursor_row = self.place(keys, cursor_x, cursor_y, on_alt);
        Outcome {
            reply: reply(keys.quiet, true, &id_key(keys), "OK"),
            cursor_row,
        }
    }

    fn query(&self, keys: &Keys) -> Outcome {
        if keys.has_id && self.images.iter().all(|image| image.id != keys.image_id) {
            return Outcome {
                reply: reply(keys.quiet, false, &format!("a=q,{}", id_key(keys)), "ENOENT"),
                cursor_row: None,
            };
        }
        let extra = if keys.has_id {
            format!("a=q,{}", id_key(keys))
        } else {
            "a=q".to_string()
        };
        Outcome {
            reply: reply(keys.quiet, true, &extra, "OK"),
            cursor_row: None,
        }
    }

    fn delete(&mut self, keys: &Keys, cursor_x: usize, cursor_y: usize) -> Outcome {
        let col = cursor_x as i32;
        let row = cursor_y as i32;
        match keys.delete {
            b'a' | b'A' => {
                self.images.clear();
                self.placements.clear();
            }
            b'i' | b'I' => {
                self.images.retain(|image| image.id != keys.image_id);
                self.placements.retain(|placement| placement.image_id != keys.image_id);
            }
            b'p' => {
                self.placements.retain(|placement| {
                    !(placement.image_id == keys.image_id && placement.placement_id == keys.placement_id)
                });
                self.drop_orphans();
            }
            b'c' => {
                self.placements.retain(|placement| {
                    let inside = col >= placement.col
                        && col < placement.col + placement.cols as i32
                        && row >= placement.row
                        && row < placement.row + placement.rows as i32;
                    !inside
                });
                self.drop_orphans();
            }
            _ => {
                return Outcome {
                    reply: reply(keys.quiet, false, &id_key(keys), "EINVAL"),
                    cursor_row: None,
                };
            }
        }
        Outcome {
            reply: reply(keys.quiet, true, &id_key(keys), "OK"),
            cursor_row: None,
        }
    }

    fn place(&mut self, keys: &Keys, cursor_x: usize, cursor_y: usize, on_alt: bool) -> Option<usize> {
        let image = self.images.iter().find(|image| image.id == keys.image_id)?;
        let px_w = image.w;
        let px_h = image.h;
        let auto_c = keys.columns == 0;
        let auto_r = keys.rows == 0;
        let cols = if auto_c {
            div_ceil(px_w, self.cell_w).max(1)
        } else {
            keys.columns.max(1)
        };
        let rows = if auto_r {
            div_ceil(px_h, self.cell_h).max(1)
        } else {
            keys.rows.max(1)
        };
        let col = cursor_x as i32 + keys.x;
        let row = cursor_y as i32 + keys.y;
        if keys.placement_id != 0 {
            self.placements.retain(|placement| {
                !(placement.image_id == keys.image_id
                    && placement.placement_id == keys.placement_id
                    && placement.on_alt == on_alt)
            });
        }
        self.placements.push(Placement {
            image_id: keys.image_id,
            placement_id: keys.placement_id,
            col,
            row,
            x_px: keys.x_px,
            y_px: keys.y_px,
            px_w,
            px_h,
            cols,
            rows,
            z: keys.z,
            auto_c,
            auto_r,
            on_alt,
        });
        if keys.cursor_move == 1 {
            let next = row + rows as i32;
            Some(if next < 0 { 0 } else { next as usize })
        } else {
            None
        }
    }

    fn store_image(&mut self, id: u32, w: u32, h: u32, rgba: Vec<u8>) -> bool {
        if !edge_ok(w, h) || rgba.len() != (w as usize) * (h as usize) * 4 {
            return false;
        }
        let old = self
            .images
            .iter()
            .find(|image| image.id == id)
            .map(|image| image.rgba.len())
            .unwrap_or(0);
        let next = self.used().saturating_sub(old).saturating_add(rgba.len());
        if next > MAX_BYTES {
            return false;
        }
        if let Some(slot) = self.images.iter_mut().find(|image| image.id == id) {
            slot.w = w;
            slot.h = h;
            slot.rgba = rgba;
        } else {
            self.images.push(Image { id, w, h, rgba });
        }
        for placement in &mut self.placements {
            if placement.image_id != id {
                continue;
            }
            placement.px_w = w;
            placement.px_h = h;
            if placement.auto_c {
                placement.cols = div_ceil(w, self.cell_w).max(1);
            }
            if placement.auto_r {
                placement.rows = div_ceil(h, self.cell_h).max(1);
            }
        }
        true
    }

    fn ok_reply(&self) -> Outcome {
        Outcome {
            reply: reply(self.pending.quiet, true, &id_key(&self.pending), "OK"),
            cursor_row: None,
        }
    }

    fn err(&self, msg: &str) -> Vec<u8> {
        reply(self.pending.quiet, false, &id_key(&self.pending), msg)
    }

    fn alloc_id(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if self.images.iter().all(|image| image.id != id) {
                return id;
            }
        }
    }

    fn used(&self) -> usize {
        self.images.iter().map(|image| image.rgba.len()).sum()
    }

    fn drop_orphans(&mut self) {
        self.images.retain(|image| {
            self.placements.iter().any(|placement| placement.image_id == image.id)
        });
    }

    fn order(&self) -> Vec<usize> {
        let mut slots: Vec<usize> = (0..self.placements.len())
            .filter(|slot| self.placements[*slot].on_alt == self.showing_alt)
            .collect();
        slots.sort_by_key(|slot| (self.placements[*slot].z, *slot));
        slots
    }
}

fn id_key(keys: &Keys) -> String {
    if keys.has_id {
        format!("i={}", keys.image_id)
    } else {
        String::new()
    }
}

fn reply(quiet: u32, ok: bool, extra: &str, msg: &str) -> Vec<u8> {
    if quiet >= 2 || (quiet == 1 && ok) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(24 + extra.len() + msg.len());
    out.extend_from_slice(b"\x1b_G");
    out.extend_from_slice(extra.as_bytes());
    out.push(b';');
    out.extend_from_slice(msg.as_bytes());
    out.extend_from_slice(b"\x1b\\");
    out
}

fn split_ctrl(apc: &[u8]) -> (&[u8], &[u8]) {
    match apc.iter().position(|b| *b == b';') {
        Some(at) => (&apc[..at], &apc[at + 1..]),
        None => (apc, &[]),
    }
}

fn parse_keys(ctrl: &[u8]) -> Keys {
    let mut keys = Keys::new();
    for part in ctrl.split(|b| *b == b',') {
        let Some(eq) = part.iter().position(|b| *b == b'=') else {
            continue;
        };
        if eq == 0 {
            continue;
        }
        let key = part[0];
        let val = &part[eq + 1..];
        match key {
            b'a' => {
                if let Some(action) = val.first() {
                    keys.action = *action;
                }
            }
            b'f' => keys.format = parse_u32(val),
            b't' => {
                if let Some(medium) = val.first() {
                    keys.medium = *medium;
                }
            }
            b's' => keys.width = parse_u32(val),
            b'v' => keys.height = parse_u32(val),
            b'i' | b'I' => {
                keys.image_id = parse_u32(val);
                keys.has_id = true;
            }
            b'p' => keys.placement_id = parse_u32(val),
            b'm' => keys.more = parse_u32(val),
            b'o' => {
                if let Some(mode) = val.first() {
                    keys.compression = *mode;
                }
            }
            b'q' => {
                keys.quiet = parse_u32(val);
                keys.has_quiet = true;
            }
            b'C' => keys.cursor_move = parse_u32(val),
            b'x' => keys.x = parse_i32(val),
            b'y' => keys.y = parse_i32(val),
            b'X' => keys.x_px = parse_i32(val),
            b'Y' => keys.y_px = parse_i32(val),
            b'c' => keys.columns = parse_u32(val),
            b'r' => keys.rows = parse_u32(val),
            b'z' => keys.z = parse_i32(val),
            b'd' => {
                if let Some(mode) = val.first() {
                    keys.delete = *mode;
                }
            }
            _ => {}
        }
    }
    keys
}

fn parse_u32(bytes: &[u8]) -> u32 {
    std::str::from_utf8(bytes).ok().and_then(|text| text.parse().ok()).unwrap_or(0)
}

fn parse_i32(bytes: &[u8]) -> i32 {
    std::str::from_utf8(bytes).ok().and_then(|text| text.parse().ok()).unwrap_or(0)
}

fn strip_ws(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect()
}

fn b64_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for b in bytes {
        if *b == b'=' {
            break;
        }
        let v = val(*b)?;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub(crate) fn b64_encode(bytes: &[u8], out: &mut String) {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8) | u32::from(bytes[i + 2]);
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(ALPHA[((n >> 12) & 63) as usize] as char);
        out.push(ALPHA[((n >> 6) & 63) as usize] as char);
        out.push(ALPHA[(n & 63) as usize] as char);
        i += 3;
    }
    let rest = bytes.len() - i;
    if rest == 1 {
        let n = u32::from(bytes[i]) << 16;
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(ALPHA[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8);
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(ALPHA[((n >> 12) & 63) as usize] as char);
        out.push(ALPHA[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
}

fn inflate_zlib(data: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok()?;
    Some(out)
}

fn edge_ok(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && w <= MAX_EDGE && h <= MAX_EDGE && (w as usize).saturating_mul(h as usize).saturating_mul(4) <= MAX_BYTES
}

fn div_ceil(n: u32, d: u32) -> u32 {
    let d = d.max(1);
    n / d + u32::from(n % d != 0)
}

fn rgb_to_rgba(data: &[u8], w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !edge_ok(w, h) {
        return None;
    }
    let pixels = (w as usize) * (h as usize);
    if data.len() != pixels * 3 {
        return None;
    }
    let mut rgba = Vec::with_capacity(pixels * 4);
    for px in data.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some((w, h, rgba))
}

fn rgba_exact(data: &[u8], w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !edge_ok(w, h) {
        return None;
    }
    let pixels = (w as usize) * (h as usize);
    if data.len() != pixels * 4 {
        return None;
    }
    Some((w, h, data.to_vec()))
}

fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder.set_limits(png::Limits { bytes: MAX_BYTES });
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let width = info.width;
    let height = info.height;
    if !edge_ok(width, height) {
        return None;
    }
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let (_, _, rgba) = rgb_to_rgba(src, width, height)?;
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(src.len() * 4);
            for sample in src {
                rgba.extend_from_slice(&[*sample, *sample, *sample, 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(src.len() * 2);
            for px in src.chunks_exact(2) {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            rgba
        }
        _ => return None,
    };
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    Some((width, height, rgba))
}
