//! Standalone Wawona Terminal (`nix run .#terminal`).
//! Grid: libghostty-rs [`TerminalSession`]. Host paints cells (GTK on Linux).

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

#[cfg(target_os = "macos")]
fn main() {
    eprintln!(
        "Wawona Terminal macOS GUI: use the AppKit Metal host.\n\
         From this repo: ./scripts/run-standalone-macos.sh\n\
         Or: nix run .#terminal (builds + opens the .app)"
    );
    std::process::exit(2);
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("Wawona Terminal standalone: Linux or macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::process::{Command, Stdio};
    use std::rc::Rc;
    use std::time::Duration;

    use gtk::gdk;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use wawona_terminal::TerminalSession;

    pub fn run() {
        let app = gtk::Application::builder()
            .application_id("com.aspauldingcode.wawona.terminal")
            .build();
        app.connect_activate(|app| {
            if let Err(err) = present(app) {
                eprintln!("Wawona Terminal: {err}");
                app.quit();
            }
        });
        let code = app.run();
        std::process::exit(code.into());
    }

    fn present(app: &gtk::Application) -> Result<(), String> {
        let (master_fd, slave_fd) = open_pty()?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        let stdin = slave
            .try_clone()
            .map_err(|e| e.to_string())
            .map(Stdio::from)?;
        let stdout = slave
            .try_clone()
            .map_err(|e| e.to_string())
            .map(Stdio::from)?;
        let child = Command::new(&shell)
            .arg("-l")
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::from(slave))
            .env("TERM", "xterm-256color")
            .spawn()
            .map_err(|e| format!("shell: {e}"))?;
        // Keep child alive for the GTK main loop (do not wait).
        let _child = Rc::new(RefCell::new(child));

        let session = TerminalSession::new(100, 32);
        let master = Rc::new(RefCell::new(Some(unsafe { File::from_raw_fd(master_fd) })));

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Wawona Terminal")
            .default_width(900)
            .default_height(560)
            .build();

        let view = gtk::TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_monospace(true);
        view.set_wrap_mode(gtk::WrapMode::None);
        view.set_top_margin(10);
        view.set_bottom_margin(10);
        view.set_left_margin(12);
        view.set_right_margin(12);
        let css = gtk::CssProvider::new();
        css.load_from_data(
            b"
            textview {
              background-color: #000000;
              color: #e8e8e8;
              font-family: monospace;
              font-size: 13pt;
            }
            ",
        );
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("display"),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let scroll = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .build();
        window.set_child(Some(&scroll));

        // Banner proves libghostty-rs path.
        session.feed(
            b"\x1b[1;36mWawona Terminal\x1b[0m  standalone · libghostty-rs · GTK\r\n".to_vec(),
        );
        paint(&view, &session);

        let session_r = session.clone();
        let master_r = master.clone();
        let view_r = view.clone();
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let mut buf = [0u8; 8192];
            let mut fed = false;
            if let Some(file) = master_r.borrow_mut().as_mut() {
                loop {
                    match file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            session_r.feed(buf[..n].to_vec());
                            fed = true;
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                let reply = session_r.take_reply();
                if !reply.is_empty() {
                    let _ = file.write_all(&reply);
                }
            }
            if fed {
                paint(&view_r, &session_r);
            }
            glib::ControlFlow::Continue
        });

        let session_k = session.clone();
        let master_k = master.clone();
        let controller = gtk::EventControllerKey::new();
        controller.connect_key_pressed(move |_, key, _code, _mods| {
            let bytes = key_bytes(key);
            if bytes.is_empty() {
                return glib::Propagation::Proceed;
            }
            if let Some(file) = master_k.borrow_mut().as_mut() {
                let _ = file.write_all(&bytes);
            } else {
                session_k.feed(bytes);
            }
            glib::Propagation::Stop
        });
        view.add_controller(controller);

        // Non-blocking master.
        set_nonblock(master.borrow().as_ref().map(|f| f.as_raw_fd()).unwrap_or(-1));

        window.present();
        Ok(())
    }

    fn paint(view: &gtk::TextView, session: &TerminalSession) {
        let buffer = view.buffer();
        let tag_table = buffer.tag_table();
        buffer.set_text("");
        let mut end = buffer.end_iter();
        for line in session.lines() {
            for span in line.spans {
                let start_off = end.offset();
                buffer.insert(&mut end, &span.text);
                let mut start = buffer.iter_at_offset(start_off);
                let tag = color_tag(&tag_table, span.fg, span.bg, span.bold);
                buffer.apply_tag(&tag, &start, &end);
                // Keep `end` after insert (mutably advanced above).
                let _ = start;
            }
            buffer.insert(&mut end, "\n");
        }
        buffer.place_cursor(&mut end);
    }

    fn color_tag(
        table: &gtk::TextTagTable,
        fg: u32,
        bg: u32,
        bold: bool,
    ) -> gtk::TextTag {
        let name = format!("c-{fg:06x}-{bg:06x}-{}", if bold { "b" } else { "n" });
        if let Some(existing) = table.lookup(&name) {
            return existing;
        }
        let tag = gtk::TextTag::new(Some(&name));
        tag.set_foreground_rgba(Some(&gdk::RGBA::new(
            ((fg >> 16) & 0xff) as f32 / 255.0,
            ((fg >> 8) & 0xff) as f32 / 255.0,
            (fg & 0xff) as f32 / 255.0,
            1.0,
        )));
        if bg != 0 {
            tag.set_background_rgba(Some(&gdk::RGBA::new(
                ((bg >> 16) & 0xff) as f32 / 255.0,
                ((bg >> 8) & 0xff) as f32 / 255.0,
                (bg & 0xff) as f32 / 255.0,
                1.0,
            )));
        }
        if bold {
            tag.set_weight(700);
        }
        table.add(&tag);
        tag
    }

    fn key_bytes(key: gdk::Key) -> Vec<u8> {
        match key {
            gdk::Key::Return | gdk::Key::KP_Enter => vec![0x0d],
            gdk::Key::BackSpace => vec![0x7f],
            gdk::Key::Tab => vec![0x09],
            gdk::Key::Escape => vec![0x1b],
            gdk::Key::Up => b"\x1b[A".to_vec(),
            gdk::Key::Down => b"\x1b[B".to_vec(),
            gdk::Key::Right => b"\x1b[C".to_vec(),
            gdk::Key::Left => b"\x1b[D".to_vec(),
            other => other
                .to_unicode()
                .map(|c| {
                    let mut buf = [0u8; 4];
                    c.encode_utf8(&mut buf).as_bytes().to_vec()
                })
                .unwrap_or_default(),
        }
    }

    fn open_pty() -> Result<(i32, i32), String> {
        let mut master = -1;
        let mut slave = -1;
        let rc = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if rc != 0 {
            return Err("openpty failed".into());
        }
        Ok((master, slave))
    }

    fn set_nonblock(fd: i32) {
        if fd < 0 {
            return;
        }
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
    }
}
