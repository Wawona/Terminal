//! Cell layout shared by C hosts (`ghostty_surface_viewport`) and Metal paint.

/// One glyph cell. Layout must stay ABI-stable for AppKit Metal hosts.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WwnTermCell {
    pub cp: u32,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u32,
}

pub const ATTR_BOLD: u32 = 1;
pub const ATTR_DIM: u32 = 2;
pub const ATTR_ITALIC: u32 = 4;
pub const ATTR_UNDERLINE: u32 = 8;
pub const ATTR_BLINK: u32 = 16;
pub const ATTR_STRIKE: u32 = 32;

#[inline]
pub fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}
