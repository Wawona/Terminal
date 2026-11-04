#if os(macOS)
import AppKit
import Darwin
import Metal
import QuartzCore

/// macOS Terminal paint host for Start/Focus.
/// Product path: `TerminalMetalPresent` (Rust VT cells → Metal atlas).
/// Zig `ghostty_surface_*` is retired from this host (bridge symbols remain
/// for linkers that still export them; they are never engaged here).
final class GhosttySurfaceHost: NSView {
    private let gpu = TerminalMetalPresent(frame: .zero, device: nil)
    /// Always false. Kept so callers that read Zig preference stay compile-clean.
    private(set) var usesZigPaint = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        gpu.autoresizingMask = [.width, .height]
        gpu.frame = bounds
        addSubview(gpu)
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("unused") }

    override var acceptsFirstResponder: Bool { false }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    var drawableSize: CGSize {
        get { gpu.drawableSize }
        set { gpu.drawableSize = newValue }
    }

    override var layer: CALayer? {
        get { gpu.layer }
        set { gpu.layer = newValue }
    }

    func syncBackingScale(_ scale: CGFloat) {
        gpu.syncBackingScale(scale)
    }

    func present(
        _ grid: [WwnTermCell], count: Int, cols: Int, rows: Int, blinkOn: Bool,
        cellW: CGFloat, cellH: CGFloat
    ) {
        present(
            grid, count: count, cols: cols, rows: rows, blinkOn: blinkOn,
            pictures: [], cellW: cellW, cellH: cellH,
            selection: nil, cursorJumpFrom: nil, cursorJumpTo: nil, cursorJumpT: 0
        )
    }

    func present(
        _ grid: [WwnTermCell], count: Int, cols: Int, rows: Int, blinkOn: Bool,
        pictures: [TermPicture], cellW: CGFloat, cellH: CGFloat,
        selection: (start: Int, end: Int)?,
        cursorJumpFrom: (col: Int, row: Int)?,
        cursorJumpTo: (col: Int, row: Int)?,
        cursorJumpT: Float
    ) {
        gpu.present(
            grid, count: count, cols: cols, rows: rows, blinkOn: blinkOn,
            pictures: pictures, cellW: cellW, cellH: cellH,
            selection: selection, cursorJumpFrom: cursorJumpFrom,
            cursorJumpTo: cursorJumpTo, cursorJumpT: cursorJumpT
        )
    }

    override func layout() {
        super.layout()
        gpu.frame = bounds
    }
}
#endif
