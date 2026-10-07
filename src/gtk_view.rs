//! GTK text view for Wawona Terminal on Linux.
//!
//! The session owns the cells. This file only fills a `TextBuffer`.

use crate::{TerminalSession, TerminalSpan};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextTag};

/// One VT row is one visual row. GTK must not soft-wrap a full-width line.
pub fn configure_view(view: &gtk4::TextView) {
    view.set_wrap_mode(gtk4::WrapMode::None);
    view.set_monospace(true);
    view.set_editable(false);
}

pub fn fill_buffer(session: &TerminalSession, buffer: &TextBuffer) {
    buffer.set_text("");
    let table = buffer.tag_table();
    for line in session.lines() {
        for span in line.spans {
            let tag = tag_for(buffer, &table, &span);
            let mut end = buffer.end_iter();
            buffer.insert_with_tags(&mut end, &span.text, &[tag]);
        }
        let mut end = buffer.end_iter();
        buffer.insert(&mut end, "\n");
    }
}

fn tag_for(buffer: &TextBuffer, table: &gtk4::TextTagTable, span: &TerminalSpan) -> TextTag {
    let name = format!(
        "wtn-{:06x}-{:06x}-{}{}{}",
        span.fg & 0xFFFFFF,
        span.bg & 0xFFFFFF,
        u8::from(span.bold),
        u8::from(span.italic),
        u8::from(span.underline)
    );
    if let Some(tag) = table.lookup(&name) {
        return tag;
    }
    let tag = buffer.create_tag(Some(&name), &[]);
    tag.set_foreground(Some(&css(span.fg)));
    if span.bg != 0 {
        tag.set_background(Some(&css(span.bg)));
    }
    if span.bold {
        tag.set_weight(700);
    }
    if span.italic {
        tag.set_style(gtk4::pango::Style::Italic);
    }
    if span.underline {
        tag.set_underline(gtk4::pango::Underline::Single);
    }
    tag
}

fn css(rgb: u32) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgb >> 16) & 0xFF,
        (rgb >> 8) & 0xFF,
        rgb & 0xFF
    )
}
