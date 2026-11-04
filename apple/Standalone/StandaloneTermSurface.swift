#if os(macOS)
import AppKit
import CoreText
import Darwin
import Metal
import MetalKit

// Metal GPU: apple/Terminal/GhosttyGpuView.swift (canonical interim atlas).
// Compiled together by scripts/run-standalone-macos.sh.

// MARK: - C ABI (libghostty-rs)

@_silgen_name("wawona_pty_spawn_shell")
private func wawona_pty_spawn_shell(_ master: UnsafeMutablePointer<Int32>, _ shell: UnsafePointer<CChar>?) -> pid_t

@_silgen_name("wwn_term_new")
private func wwn_term_new(_ cols: UInt32, _ rows: UInt32) -> OpaquePointer?
@_silgen_name("wwn_term_free")
private func wwn_term_free(_ term: OpaquePointer?)
@_silgen_name("wwn_term_resize")
private func wwn_term_resize(_ term: OpaquePointer?, _ cols: UInt32, _ rows: UInt32)
@_silgen_name("wwn_term_feed")
private func wwn_term_feed(_ term: OpaquePointer?, _ bytes: UnsafePointer<UInt8>?, _ len: Int)
@_silgen_name("wwn_term_viewport")
private func wwn_term_viewport(
    _ term: OpaquePointer?, _ origin: UInt32, _ out: UnsafeMutablePointer<WwnTermCell>?, _ cap: UInt32,
    _ cols: UnsafeMutablePointer<UInt32>?, _ rows: UnsafeMutablePointer<UInt32>?
) -> UInt32
@_silgen_name("wwn_term_origin")
private func wwn_term_origin(_ term: OpaquePointer?) -> UInt32
@_silgen_name("wwn_term_take_reply")
private func wwn_term_take_reply(
    _ term: OpaquePointer?, _ buf: UnsafeMutablePointer<UInt8>?, _ cap: Int
) -> Int
@_silgen_name("wwn_term_pointer")
private func wwn_term_pointer(
    _ term: OpaquePointer?, _ button: UInt8, _ col: UInt32, _ row: UInt32, _ pressed: Int32, _ motion: Int32
)
@_silgen_name("wwn_term_set_cell_px")
private func wwn_term_set_cell_px(_ term: OpaquePointer?, _ width: UInt32, _ height: UInt32)

private func ghosttyViewport(
    _ term: OpaquePointer, origin: UInt32, cols: inout UInt32, rows: inout UInt32
) -> [WwnTermCell] {
    var outCols: UInt32 = 0
    var outRows: UInt32 = 0
    let probeCap: UInt32 = 512 * 256
    var storage = [WwnTermCell](
        repeating: WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0),
        count: Int(probeCap)
    )
    let n = storage.withUnsafeMutableBufferPointer { buf -> UInt32 in
        wwn_term_viewport(
            term, origin, buf.baseAddress, probeCap, &outCols, &outRows
        )
    }
    cols = outCols
    rows = outRows
    let count = Int(n)
    guard count > 0, count <= storage.count else { return [] }
    return Array(storage.prefix(count))
}

// MARK: - Host surface + PTY

final class StandaloneTermSurface: NSView {
    var onReply: ((Data) -> Void)?
    var onResize: ((UInt32, UInt32) -> Void)?

    private var term: OpaquePointer?
    private let gpu = GhosttyGpuView(frame: .zero, device: nil)
    private var grid = [WwnTermCell](repeating: WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0), count: 512 * 32)
    private var cols: UInt32 = 100
    private var rows: UInt32 = 32
    private var blinkOn = true
    private var blinkTimer: Timer?
    private let pointSize: CGFloat = 13
    private var cellPxW: CGFloat = 8
    private var cellPxH: CGFloat = 16

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        term = wwn_term_new(cols, rows)
        gpu.autoresizingMask = [.width, .height]
        gpu.frame = bounds
        addSubview(gpu)
        blinkTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.blinkOn.toggle()
            self.redraw()
        }
        let banner = "\u{1b}[1;36mWawona Terminal\u{1b}[0m  standalone · libghostty-rs · Metal\r\n"
        feed(Data(banner.utf8))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    deinit {
        blinkTimer?.invalidate()
        if let term { wwn_term_free(term) }
    }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { bounds.contains(point) ? self : nil }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    func feed(_ bytes: Data) {
        guard let term, !bytes.isEmpty else { return }
        bytes.withUnsafeBytes { raw in
            guard let base = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
            wwn_term_feed(term, base, bytes.count)
        }
        flushReply()
        redraw()
    }

    override func layout() {
        super.layout()
        gpu.frame = bounds
        syncDrawableScale()
        fit()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        syncDrawableScale()
    }

    private func syncDrawableScale() {
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        gpu.layer?.contentsScale = scale
        gpu.syncBackingScale(scale)
        let w = max(1, bounds.width * scale)
        let h = max(1, bounds.height * scale)
        gpu.drawableSize = CGSize(width: w, height: h)
    }

    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command) {
            super.keyDown(with: event)
            return
        }
        let chars = event.characters ?? event.charactersIgnoringModifiers ?? ""
        if event.keyCode == 36 || event.keyCode == 76 {
            onReply?(Data([0x0D]))
            return
        }
        if event.keyCode == 51 {
            onReply?(Data([0x7F]))
            return
        }
        if !chars.isEmpty {
            onReply?(Data(chars.utf8))
        }
    }

    private func fit() {
        guard let term, bounds.width > 1, bounds.height > 1 else { return }
        let font = NSFont.monospacedSystemFont(ofSize: pointSize, weight: .regular)
        let cellW = max(1, ("M" as NSString).size(withAttributes: [.font: font]).width)
        let cellH = max(1, font.ascender - font.descender + font.leading)
        let nextCols = UInt32(min(512, max(20, Int((bounds.width - 16) / cellW))))
        let nextRows = UInt32(min(256, max(8, Int(bounds.height / cellH))))
        cellPxW = cellW
        cellPxH = cellH
        wwn_term_set_cell_px(term, UInt32(max(1, cellW.rounded())), UInt32(max(1, cellH.rounded())))
        guard nextCols != cols || nextRows != rows else {
            redraw()
            return
        }
        cols = nextCols
        rows = nextRows
        wwn_term_resize(term, cols, rows)
        onResize?(cols, rows)
        redraw()
    }

    private func flushReply() {
        guard let term else { return }
        var buf = [UInt8](repeating: 0, count: 256)
        let n = buf.withUnsafeMutableBytes { raw in
            wwn_term_take_reply(term, raw.baseAddress?.assumingMemoryBound(to: UInt8.self), raw.count)
        }
        if n > 0 { onReply?(Data(buf.prefix(n))) }
    }

    private func redraw() {
        guard let term else { return }
        var outCols: UInt32 = 0
        var outRows: UInt32 = 0
        let cells = ghosttyViewport(
            term, origin: wwn_term_origin(term), cols: &outCols, rows: &outRows
        )
        if outCols > 0 { cols = outCols }
        if outRows > 0 { rows = outRows }
        grid = cells.isEmpty
            ? [WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0)]
            : cells
        gpu.present(
            grid, count: grid.count, cols: Int(cols), rows: Int(rows), blinkOn: blinkOn,
            cellW: cellPxW, cellH: cellPxH
        )
    }
}

final class StandaloneAppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var surface: StandaloneTermSurface?
    private var master: Int32 = -1
    private var child: pid_t = -1
    private var reader: DispatchSourceRead?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let surface = StandaloneTermSurface(frame: NSRect(x: 0, y: 0, width: 900, height: 560))
        surface.onReply = { [weak self] data in
            self?.writeMaster(data)
        }
        surface.onResize = { [weak self] cols, rows in
            self?.setWinsize(cols: cols, rows: rows)
        }
        self.surface = surface

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 560),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.title = "Wawona Terminal"
        window.contentView = surface
        window.center()
        window.makeKeyAndOrderFront(nil)
        self.window = window

        startShell()
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func startShell() {
        var m: Int32 = -1
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        let pid = shell.withCString { path in
            wawona_pty_spawn_shell(&m, path)
        }
        guard pid > 0, m >= 0 else { return }
        master = m
        child = pid
        let flags = fcntl(master, F_GETFL)
        _ = fcntl(master, F_SETFL, flags | O_NONBLOCK)

        let source = DispatchSource.makeReadSource(fileDescriptor: master, queue: .main)
        source.setEventHandler { [weak self] in
            self?.drainMaster()
        }
        source.resume()
        reader = source
    }

    private func drainMaster() {
        guard master >= 0 else { return }
        var buf = [UInt8](repeating: 0, count: 8192)
        while true {
            let n = read(master, &buf, buf.count)
            if n > 0 {
                surface?.feed(Data(buf.prefix(n)))
            } else {
                break
            }
        }
    }

    private func writeMaster(_ data: Data) {
        guard master >= 0, !data.isEmpty else { return }
        data.withUnsafeBytes { raw in
            if let base = raw.baseAddress {
                _ = write(master, base, data.count)
            }
        }
    }

    private func setWinsize(cols: UInt32, rows: UInt32) {
        let fd = master
        guard fd >= 0 else { return }
        var win = winsize(
            ws_row: UInt16(min(rows, UInt32(UInt16.max))),
            ws_col: UInt16(min(cols, UInt32(UInt16.max))),
            ws_xpixel: 0,
            ws_ypixel: 0
        )
        _ = ioctl(fd, TIOCSWINSZ, &win)
    }
}

@main
enum StandaloneMain {
    static func main() {
        let app = NSApplication.shared
        let delegate = StandaloneAppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
#endif
