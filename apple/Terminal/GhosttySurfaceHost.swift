#if os(macOS)
import AppKit
import Darwin
import Metal
import QuartzCore

/// macOS Terminal paint host. Phase 5: when Zig `libghostty-internal.a` is
/// linked (`WAWONA_LINK_LIBGHOSTTY=1` → `WAWONA_HAS_LIBGHOSTTY`), paints via
/// `ghostty_app_new` + `ghostty_surface_new(nsview)` + `ghostty_surface_draw`.
/// Otherwise embeds interim `GhosttyGpuView` atlas (not retired as default).
final class GhosttySurfaceHost: NSView {
    private let gpuFallback = GhosttyGpuView(frame: .zero, device: nil)
    private var zigHost: OpaquePointer?
    private var displayLink: CVDisplayLink?
    private var zigChild: NSView?
    private(set) var usesZigPaint = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        gpuFallback.autoresizingMask = [.width, .height]
        gpuFallback.frame = bounds
        addSubview(gpuFallback)
        // Prefer Zig when tip linked the archive (compile-time bridge flag).
        if wwn_ghostty_surface_host_available() {
            usesZigPaint = true
            gpuFallback.isHidden = true
            let child = NSView(frame: bounds)
            child.autoresizingMask = [.width, .height]
            child.wantsLayer = true
            addSubview(child)
            zigChild = child
        }
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("unused") }

    deinit {
        stopDisplayLink()
        if let zigHost {
            wwn_ghostty_surface_host_free(zigHost)
        }
    }

    override var acceptsFirstResponder: Bool { false }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    var drawableSize: CGSize {
        get { gpuFallback.drawableSize }
        set { gpuFallback.drawableSize = newValue }
    }

    override var layer: CALayer? {
        get { usesZigPaint ? (zigChild?.layer ?? super.layer) : gpuFallback.layer }
        set {
            if usesZigPaint {
                zigChild?.layer = newValue
            } else {
                gpuFallback.layer = newValue
            }
        }
    }

    func syncBackingScale(_ scale: CGFloat) {
        if usesZigPaint {
            restartZigIfNeeded()
            return
        }
        gpuFallback.syncBackingScale(scale)
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
        if usesZigPaint {
            // Zig owns VT + Metal paint. wwn_term grid is not the paint source.
            wwn_ghostty_surface_host_draw(zigHost)
            return
        }
        gpuFallback.present(
            grid, count: count, cols: cols, rows: rows, blinkOn: blinkOn,
            pictures: pictures, cellW: cellW, cellH: cellH,
            selection: selection, cursorJumpFrom: cursorJumpFrom,
            cursorJumpTo: cursorJumpTo, cursorJumpT: cursorJumpT
        )
    }

    override func layout() {
        super.layout()
        gpuFallback.frame = bounds
        zigChild?.frame = bounds
        if usesZigPaint {
            restartZigIfNeeded()
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if usesZigPaint {
            restartZigIfNeeded()
        }
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        if usesZigPaint {
            restartZigIfNeeded()
        }
    }

    private func restartZigIfNeeded() {
        guard usesZigPaint, let child = zigChild, window != nil else { return }
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        let w = UInt32(max(1, (bounds.width * scale).rounded()))
        let h = UInt32(max(1, (bounds.height * scale).rounded()))
        guard w > 1, h > 1 else { return }

        if let zigHost {
            wwn_ghostty_surface_host_resize(zigHost, w, h, Double(scale))
            wwn_ghostty_surface_host_draw(zigHost)
            startDisplayLink()
            return
        }

        let started = wwn_ghostty_surface_host_start(
            Unmanaged.passUnretained(child).toOpaque(), Double(scale), w, h
        )
        guard let started else {
            // Link symbols present but surface_new failed: fall back to atlas.
            NSLog("GhosttySurfaceHost: zig start failed; GhosttyGpu fallback")
            usesZigPaint = false
            gpuFallback.isHidden = false
            child.removeFromSuperview()
            zigChild = nil
            return
        }
        zigHost = started
        NSLog("GhosttySurfaceHost: zig paint via ghostty_surface_draw")
        startDisplayLink()
    }

    private func startDisplayLink() {
        guard displayLink == nil else { return }
        var link: CVDisplayLink?
        CVDisplayLinkCreateWithActiveCGDisplays(&link)
        guard let link else { return }
        displayLink = link
        CVDisplayLinkSetOutputCallback(link, { _, _, _, _, _, ctx -> CVReturn in
            guard let ctx else { return kCVReturnSuccess }
            let host = Unmanaged<GhosttySurfaceHost>.fromOpaque(ctx).takeUnretainedValue()
            DispatchQueue.main.async {
                wwn_ghostty_surface_host_draw(host.zigHost)
            }
            return kCVReturnSuccess
        }, Unmanaged.passUnretained(self).toOpaque())
        CVDisplayLinkStart(link)
    }

    private func stopDisplayLink() {
        if let displayLink {
            CVDisplayLinkStop(displayLink)
        }
        displayLink = nil
    }
}

@_silgen_name("wwn_ghostty_surface_host_available")
private func wwn_ghostty_surface_host_available() -> Bool

@_silgen_name("wwn_ghostty_surface_host_start")
private func wwn_ghostty_surface_host_start(
    _ nsview: UnsafeMutableRawPointer?, _ scale: Double, _ width: UInt32, _ height: UInt32
) -> OpaquePointer?

@_silgen_name("wwn_ghostty_surface_host_resize")
private func wwn_ghostty_surface_host_resize(
    _ host: OpaquePointer?, _ width: UInt32, _ height: UInt32, _ scale: Double
)

@_silgen_name("wwn_ghostty_surface_host_draw")
private func wwn_ghostty_surface_host_draw(_ host: OpaquePointer?)

@_silgen_name("wwn_ghostty_surface_host_free")
private func wwn_ghostty_surface_host_free(_ host: OpaquePointer?)
#endif
