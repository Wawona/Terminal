//! C ABI for AppKit / Metal hosts (`ghostty_surface_*`, `wwn_term_*`).
//! Backed by [`super::engine::VtEngine`] (real libghostty-vt).
//! `super::` so this file also compiles as Wawona `src/term/ffi_surface.rs`.

use super::cell::WwnTermCell;
use super::engine::VtEngine;
use std::ffi::{CString, c_char};
use std::os::raw::c_void;

fn engine_mut(term: *mut c_void) -> Option<&'static mut VtEngine> {
    if term.is_null() {
        None
    } else {
        Some(unsafe { &mut *(term as *mut VtEngine) })
    }
}

fn engine_ref(term: *const c_void) -> Option<&'static VtEngine> {
    if term.is_null() {
        None
    } else {
        Some(unsafe { &*(term as *const VtEngine) })
    }
}

#[no_mangle]
pub extern "C" fn ghostty_init() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn ghostty_shutdown() {}

#[no_mangle]
pub extern "C" fn ghostty_surface_new(cols: u32, rows: u32) -> *mut c_void {
    Box::into_raw(Box::new(VtEngine::new(cols, rows))) as *mut c_void
}

#[no_mangle]
pub extern "C" fn ghostty_surface_free(s: *mut c_void) {
    if !s.is_null() {
        unsafe {
            drop(Box::from_raw(s as *mut VtEngine));
        }
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_resize(s: *mut c_void, cols: u32, rows: u32) {
    if let Some(eng) = engine_mut(s) {
        eng.resize(cols, rows);
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_write(s: *mut c_void, bytes: *const u8, len: usize) {
    let Some(eng) = engine_mut(s) else {
        return;
    };
    if bytes.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
    eng.feed(slice);
}

#[no_mangle]
pub extern "C" fn ghostty_surface_key(
    _s: *mut c_void,
    _key: u32,
    _mods: u32,
    _action: u32,
    _text: *const u8,
    _text_len: usize,
) {
    // Key encoding lands with host keymap bridge; PTY path feeds bytes today.
}

#[no_mangle]
pub extern "C" fn ghostty_surface_take_reply(s: *mut c_void, buf: *mut u8, cap: usize) -> usize {
    let Some(eng) = engine_mut(s) else {
        return 0;
    };
    let reply = eng.take_reply();
    if buf.is_null() || cap == 0 {
        return reply.len();
    }
    let n = reply.len().min(cap);
    unsafe {
        std::ptr::copy_nonoverlapping(reply.as_ptr(), buf, n);
    }
    n
}

#[no_mangle]
pub extern "C" fn ghostty_surface_viewport(
    s: *mut c_void,
    origin: u32,
    out: *mut WwnTermCell,
    cap: u32,
    cols_out: *mut u32,
    rows_out: *mut u32,
) -> u32 {
    let Some(eng) = engine_mut(s) else {
        return 0;
    };
    if out.is_null() || cols_out.is_null() || rows_out.is_null() {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(out, cap as usize) };
    let (n, cols, rows) = eng.write_viewport(origin, slice);
    unsafe {
        *cols_out = cols;
        *rows_out = rows;
    }
    n
}

#[no_mangle]
pub extern "C" fn ghostty_surface_origin(s: *const c_void) -> u32 {
    engine_ref(s).map(|e| e.origin()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn ghostty_surface_scroll_to(s: *mut c_void, origin: u32) {
    if let Some(eng) = engine_mut(s) {
        eng.scroll_to_origin(origin);
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_scroll_delta(s: *mut c_void, delta: i32) {
    if let Some(eng) = engine_mut(s) {
        eng.scroll_delta(delta);
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_scroll_bottom(s: *mut c_void) {
    if let Some(eng) = engine_mut(s) {
        eng.scroll_to_bottom();
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_scroll_total(s: *const c_void) -> u32 {
    engine_ref(s).map(|e| e.scroll_total()).unwrap_or(0)
}

/// Copy Ghostty active selection plain text into `out` (NUL-terminated when
/// `cap > 0`). Returns byte length without NUL. Empty when no selection.
/// Hosts must install selection via Ghostty; this does not invent one.
#[no_mangle]
pub extern "C" fn ghostty_surface_selection_text(s: *const c_void, out: *mut u8, cap: u32) -> u32 {
    let Some(eng) = engine_ref(s) else {
        return 0;
    };
    let text = eng.selection_text();
    let bytes = text.as_bytes();
    if out.is_null() || cap == 0 {
        return bytes.len() as u32;
    }
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
pub extern "C" fn ghostty_surface_title(s: *const c_void, out: *mut u8, cap: u32) -> u32 {
    let Some(eng) = engine_ref(s) else {
        return 0;
    };
    if out.is_null() || cap == 0 {
        return 0;
    }
    let bytes = eng.title().into_bytes();
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
pub extern "C" fn ghostty_surface_set_cell_px(s: *mut c_void, width: u32, height: u32) {
    if let Some(eng) = engine_mut(s) {
        eng.set_cell_px(width, height);
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_pointer(
    s: *mut c_void,
    button: u8,
    col: u32,
    row: u32,
    pressed: i32,
    motion: i32,
) {
    // i32 matches AppKit Swift @_silgen_name (bool is not C ABI-stable here).
    if let Some(eng) = engine_mut(s) {
        eng.pointer(button, col, row, pressed != 0, motion != 0);
    }
}

#[no_mangle]
pub extern "C" fn ghostty_surface_encode_paste(
    s: *mut c_void,
    text: *const u8,
    len: usize,
) -> *mut c_char {
    let Some(eng) = engine_mut(s) else {
        return std::ptr::null_mut();
    };
    let slice = if text.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    let owned = String::from_utf8_lossy(slice).into_owned();
    let encoded = eng.encode_paste(&owned);
    match CString::new(encoded) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn ghostty_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

// --- wwn_term_* aliases (same engine) ---

#[no_mangle]
pub extern "C" fn wwn_term_new(cols: u32, rows: u32) -> *mut c_void {
    ghostty_surface_new(cols, rows)
}

#[no_mangle]
pub extern "C" fn wwn_term_free(term: *mut c_void) {
    ghostty_surface_free(term)
}

#[no_mangle]
pub extern "C" fn wwn_term_resize(term: *mut c_void, cols: u32, rows: u32) {
    ghostty_surface_resize(term, cols, rows)
}

#[no_mangle]
pub extern "C" fn wwn_term_feed(term: *mut c_void, bytes: *const u8, len: usize) {
    ghostty_surface_write(term, bytes, len)
}

#[no_mangle]
pub extern "C" fn wwn_term_take_reply(term: *mut c_void, buf: *mut u8, cap: usize) -> usize {
    ghostty_surface_take_reply(term, buf, cap)
}

#[no_mangle]
pub extern "C" fn wwn_term_cols(term: *const c_void) -> u32 {
    engine_ref(term).map(|e| e.cols()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn wwn_term_line_count(term: *const c_void) -> u32 {
    engine_ref(term).map(|e| e.rows()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn wwn_term_viewport(
    term: *const c_void,
    origin: u32,
    out: *mut WwnTermCell,
    cap: u32,
    cols_out: *mut u32,
    rows_out: *mut u32,
) -> u32 {
    ghostty_surface_viewport(term as *mut c_void, origin, out, cap, cols_out, rows_out)
}

#[no_mangle]
pub extern "C" fn wwn_term_origin(term: *const c_void) -> u32 {
    ghostty_surface_origin(term)
}

#[no_mangle]
pub extern "C" fn wwn_term_scroll_to(term: *mut c_void, origin: u32) {
    ghostty_surface_scroll_to(term, origin)
}

#[no_mangle]
pub extern "C" fn wwn_term_scroll_delta(term: *mut c_void, delta: i32) {
    ghostty_surface_scroll_delta(term, delta)
}

#[no_mangle]
pub extern "C" fn wwn_term_scroll_bottom(term: *mut c_void) {
    ghostty_surface_scroll_bottom(term)
}

#[no_mangle]
pub extern "C" fn wwn_term_scroll_total(term: *const c_void) -> u32 {
    ghostty_surface_scroll_total(term)
}

#[no_mangle]
pub extern "C" fn wwn_term_selection_text(term: *const c_void, out: *mut u8, cap: u32) -> u32 {
    ghostty_surface_selection_text(term, out, cap)
}

#[no_mangle]
pub extern "C" fn wwn_term_title(term: *const c_void, out: *mut u8, cap: u32) -> u32 {
    ghostty_surface_title(term, out, cap)
}

#[no_mangle]
pub extern "C" fn wwn_term_set_cell_px(term: *mut c_void, width: u32, height: u32) {
    ghostty_surface_set_cell_px(term, width, height)
}

#[no_mangle]
pub extern "C" fn wwn_term_refresh_scrollback_cap(term: *mut c_void, host_factor_permille: u32) {
    let Some(eng) = engine_mut(term) else {
        return;
    };
    let factor = (host_factor_permille.clamp(250, 1000) as f32) / 1000.0;
    let cols = eng.cols();
    let base = super::engine::recommended_scroll_cap(cols);
    let capped = ((base as f32) * factor).round() as usize;
    eng.set_scroll_cap(capped.clamp(1_000, 100_000));
}

#[no_mangle]
pub extern "C" fn wwn_term_image_count(_term: *const c_void) -> u32 {
    0
}

#[repr(C)]
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

#[no_mangle]
pub extern "C" fn wwn_term_image_meta(
    _term: *const c_void,
    _index: u32,
    _out: *mut WwnTermImage,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn wwn_term_image_rgba(
    _term: *const c_void,
    _index: u32,
    _out: *mut u8,
    _cap: u32,
) -> u32 {
    0
}

fn cstring_owned(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn wwn_term_string_free(ptr: *mut c_char) {
    ghostty_string_free(ptr)
}

/// UniFFI-shaped lines JSON for a live term. Caller frees with
/// `wwn_term_string_free`.
#[no_mangle]
pub extern "C" fn wwn_term_lines_json(term: *const c_void) -> *mut c_char {
    let Some(eng) = engine_ref(term) else {
        return std::ptr::null_mut();
    };
    cstring_owned(eng.lines_json())
}

/// One-shot: feed `bytes` into a throwaway VT of `cols`×`rows`, return lines
/// JSON. Watch Terminal / Android guest console use this for off-session paint.
#[no_mangle]
pub extern "C" fn wwn_term_bytes_lines_json(
    bytes: *const u8,
    len: usize,
    cols: u32,
    rows: u32,
) -> *mut c_char {
    let eng = VtEngine::new(cols.max(1), rows.max(1));
    if !bytes.is_null() && len > 0 {
        let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
        eng.feed(slice);
    }
    cstring_owned(eng.lines_json())
}

#[no_mangle]
pub extern "C" fn wwn_term_images_json(_term: *const c_void) -> *mut c_char {
    // Kitty placements empty until real VT image path lands on this target.
    cstring_owned("[]".into())
}

#[no_mangle]
pub extern "C" fn wwn_term_bytes_images_json(
    bytes: *const u8,
    len: usize,
    cols: u32,
    rows: u32,
    _cell_w: u32,
    _cell_h: u32,
) -> *mut c_char {
    // Feed so OSC graphics side effects run when VT supports them; JSON still
    // empty on stub / current Ghostty path (`placed_images` is empty).
    let eng = VtEngine::new(cols.max(1), rows.max(1));
    if !bytes.is_null() && len > 0 {
        let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
        eng.feed(slice);
    }
    let _ = eng.placed_images();
    cstring_owned("[]".into())
}
